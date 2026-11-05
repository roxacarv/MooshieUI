//! Anima multi-character workflow.
//!
//! Each character gets its own prompt, an optional LoRA and a slice of the
//! canvas, and the RS MultiCharacter Sampler custom node pack (pinned in
//! `comfyui/nodes.rs`) routes them so attributes and LoRAs stay with the
//! character they belong to. Two to six characters, in columns by default or in
//! boxes the user places.

use serde_json::{json, Value};

use super::{load_model_nodes, strip_lora_tags, WorkflowResult};
use crate::comfyui::types::{GenerationParams, MultiCharacterParams};

/// Every RS builder node requires all ten slots, whether they are used or not.
const SLOTS: usize = 10;

/// How many characters one image may have. The pack allows ten regions; the
/// app stops at six (the most NovelAI V4 allows) until crowding has been
/// measured further.
pub const MIN_CHARACTERS: usize = 2;
pub const MAX_CHARACTERS: usize = 6;

/// Defaults of `initial_spatial_separation` / `initial_character_gain` on the
/// sampler; `cohesion` moves them toward the values that blended scenes best.
const SEPARATION_STRICT: f64 = 0.9;
const SEPARATION_COHESIVE: f64 = 0.5;
const GAIN_STRICT: f64 = 1.7;
const GAIN_COHESIVE: f64 = 1.3;

/// Whether this request runs through the multi-character workflow.
pub fn active(params: &GenerationParams) -> bool {
    params.mode == "txt2img"
        && params.model_architecture == "anima"
        && params
            .multi_character
            .as_ref()
            .is_some_and(|multi| multi.enabled)
}

/// Reject what the first iteration cannot honor, rather than dropping it
/// without a word.
pub fn validate(params: &GenerationParams) -> Result<(), String> {
    let Some(multi) = params.multi_character.as_ref().filter(|m| m.enabled) else {
        return Ok(());
    };

    if params.mode != "txt2img" || params.model_architecture != "anima" {
        return Err(
            "Multi-character generation is only available for Anima models in text-to-image mode."
                .into(),
        );
    }
    if !(MIN_CHARACTERS..=MAX_CHARACTERS).contains(&multi.characters.len()) {
        return Err(format!(
            "Multi-character generation needs between {MIN_CHARACTERS} and {MAX_CHARACTERS} characters."
        ));
    }
    for (index, character) in multi.characters.iter().enumerate() {
        if character.prompt.trim().is_empty() {
            return Err(format!("Character {} needs a description.", index + 1));
        }
        if let Some(r) = character.region {
            let values = [r.x, r.y, r.width, r.height];
            let sides_ok = r.width >= MIN_REGION_SIDE && r.height >= MIN_REGION_SIDE;
            let inside = r.x - r.width / 2.0 >= -0.001
                && r.x + r.width / 2.0 <= 1.001
                && r.y - r.height / 2.0 >= -0.001
                && r.y + r.height / 2.0 <= 1.001;
            if !values.iter().all(|v| v.is_finite()) || !sides_ok || !inside {
                return Err(format!(
                    "Character {} has a position box that is too small or leaves the image.",
                    index + 1
                ));
            }
        }
        if let Some(lora) = character.lora.as_ref() {
            if lora.name.trim().is_empty() {
                return Err(format!(
                    "Character {} has a LoRA without a file.",
                    index + 1
                ));
            }
        }
    }
    if !matches!(multi.lora_mode.as_str(), "precision" | "normal") {
        return Err(format!(
            "Unknown multi-character LoRA mode '{}'.",
            multi.lora_mode
        ));
    }
    if params.batch_size != 1 {
        return Err(
            "Multi-character generation makes one image at a time: set the batch size to 1.".into(),
        );
    }

    let unsupported = [
        (
            params.controlnet.as_ref().is_some_and(|c| c.enabled),
            "ControlNet",
        ),
        (params.style_transfer_enabled, "style transfer"),
        (params.style_ref_enabled, "the style or character reference"),
        (!params.positive_regions.is_empty(), "regional prompts"),
        (!params.positive_segments.is_empty(), "prompt scheduling"),
        (
            params.pause_at_step.is_some() || !params.resume_stages.is_empty(),
            "pause and continue",
        ),
    ];
    if let Some((_, name)) = unsupported.iter().find(|(used, _)| *used) {
        return Err(format!(
            "Multi-character generation cannot be combined with {name} yet. Turn it off or disable multi-character."
        ));
    }
    Ok(())
}

fn add_node(
    workflow: &mut serde_json::Map<String, Value>,
    next_id: &mut u32,
    node: Value,
) -> String {
    let id = next_id.to_string();
    workflow.insert(id.clone(), node);
    *next_id += 1;
    id
}

/// Box centre and size for character `index` of `count`, left to right, each
/// owning an equal full-height column.
fn column(index: usize, count: usize) -> (f64, f64, f64, f64) {
    let width = 1.0 / count as f64;
    ((index as f64 + 0.5) * width, 0.5, width, 1.0)
}

/// Smallest box side the sampler can place; below it the region may vanish at
/// the model's coarse resolution.
const MIN_REGION_SIDE: f64 = 0.1;

/// The overlapping layout used while characters interact. Measured with two
/// characters: boxes 0.65 wide centred at 0.35 and 0.65 gave a connected pair,
/// where two plain columns gave a hugging pair in each half.
fn overlapping_box(index: usize, count: usize) -> (f64, f64, f64, f64) {
    if count == 2 {
        (if index == 0 { 0.35 } else { 0.65 }, 0.5, 0.65, 1.0)
    } else {
        // Same idea for more: boxes 1.3 columns wide, spread edge to edge.
        let width = (1.3 / count as f64).min(1.0);
        let step = (1.0 - width) / (count as f64 - 1.0);
        (width / 2.0 + index as f64 * step, 0.5, width, 1.0)
    }
}

/// Whether characters without a box of their own overlap: when they touch, or
/// while interaction text is set.
fn overlaps(multi: &MultiCharacterParams) -> bool {
    multi.touch || !interaction_text(multi).is_empty()
}

/// What the characters do together, trimmed; empty when not interacting.
fn interaction_text(multi: &MultiCharacterParams) -> String {
    strip_lora_tags(&multi.interaction).trim().to_string()
}

/// Where character `index` stands: its own box when it has one, the
/// overlapping layout while they interact, otherwise its slot's column.
fn region_for(multi: &MultiCharacterParams, index: usize) -> (f64, f64, f64, f64) {
    let count = multi.characters.len();
    match multi.characters.get(index).and_then(|c| c.region) {
        Some(r) => (r.x, r.y, r.width, r.height),
        None if overlaps(multi) => overlapping_box(index, count),
        None => column(index, count),
    }
}

/// Words for a box centre, or nothing when it sits in the middle third.
fn position_words(x: f64) -> Option<&'static str> {
    if x < 0.4 {
        Some("left side")
    } else if x > 0.6 {
        Some("right side")
    } else {
        None
    }
}

/// Sampler defaults of `initial_common_prior` / `common_after_step3_boost`
/// (strength 0) and the values they reach at strength 1. The node keeps the
/// scene prompt at the prior for the first 40% of denoising, so with the
/// defaults a background asked for in the main prompt never shows up.
const COMMON_PRIOR_MIN: f64 = 0.05;
const COMMON_PRIOR_MAX: f64 = 0.8;
const COMMON_BOOST_MIN: f64 = 0.1;
const COMMON_BOOST_MAX: f64 = 0.43;

/// The sampler takes sides that are multiples of 16 from 64 to 4096; the app
/// allows multiples of 8 (16:9 on 1024 gives 1368 x 768, which it refused).
/// Snap to the nearest valid side; the picture comes out at most 8 px off.
fn sampler_side(side: u32) -> u32 {
    (((side + 8) / 16) * 16).clamp(64, 4096)
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

/// One axis of a box (centre, size) rounded to four decimals without leaving
/// the image. Rounding a centre alone can push an edge past 1 (four columns:
/// 0.875 + 0.125 became 0.88 + 0.125), and the sampler refuses any box that
/// leaves the image.
fn axis_inside(centre: f64, size: f64) -> (f64, f64) {
    let centre = round4(centre).clamp(0.0, 1.0);
    let size = round4(size).min(2.0 * centre).min(2.0 * (1.0 - centre));
    (centre, size)
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Sampler tuning for a 0 (strictly apart) to 1 (one continuous scene) slider.
fn cohesion_settings(multi: &MultiCharacterParams) -> (f64, f64) {
    let cohesion = if multi.cohesion.is_finite() {
        multi.cohesion.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (
        round2(SEPARATION_STRICT + (SEPARATION_COHESIVE - SEPARATION_STRICT) * cohesion),
        round2(GAIN_STRICT + (GAIN_COHESIVE - GAIN_STRICT) * cohesion),
    )
}

/// Sampler tuning for a 0 (node defaults) to 1 (scene dominates early) slider.
fn scene_settings(multi: &MultiCharacterParams) -> (f64, f64) {
    let strength = if multi.scene_strength.is_finite() {
        multi.scene_strength.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (
        round2(COMMON_PRIOR_MIN + (COMMON_PRIOR_MAX - COMMON_PRIOR_MIN) * strength),
        round2(COMMON_BOOST_MIN + (COMMON_BOOST_MAX - COMMON_BOOST_MIN) * strength),
    )
}

pub fn build(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let multi = params
        .multi_character
        .as_ref()
        .expect("multichar::build is only called when `active` is true");
    let count = multi.characters.len();

    let mut workflow = serde_json::Map::new();

    // The sampler runs its own forward passes per character; TeaCache keeps a
    // step counter inside the model patch that does not survive that.
    let mut base_params = params.clone();
    base_params.anima_teacache_enabled = false;
    let ml = load_model_nodes(&mut workflow, 1, &base_params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    let common_prompt = strip_lora_tags(&params.positive_prompt).trim().to_string();
    let negative_prompt = strip_lora_tags(&params.negative_prompt).trim().to_string();
    let character_prompts: Vec<String> = multi
        .characters
        .iter()
        .map(|c| strip_lora_tags(&c.prompt).trim().to_string())
        .collect();
    // What each column is told. The scene text is appended so both halves agree
    // on the setting; `character_prompts` stays the characters alone for the
    // refinement conditioning below, which already adds the scene once.
    let interaction = interaction_text(multi);
    let column_prompts: Vec<String> = character_prompts
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let mut full = text.clone();
            if !interaction.is_empty() {
                full = format!("{full}, {interaction}");
            }
            if multi.say_position {
                if let Some(words) = position_words(region_for(multi, index).0) {
                    full = format!("{full}, {words}");
                }
            }
            if multi.share_scene && !common_prompt.is_empty() {
                full = format!("{full}, {common_prompt}");
            }
            full
        })
        .collect();

    // Character prompts reach the pipe as STRING connections.
    let string_ids: Vec<String> = column_prompts
        .iter()
        .map(|text| {
            add_node(
                &mut workflow,
                &mut next_id,
                json!({
                    "class_type": "PrimitiveStringMultiline",
                    "inputs": { "value": text }
                }),
            )
        })
        .collect();

    let mut prompt_inputs = serde_json::Map::new();
    prompt_inputs.insert("character_count".into(), json!(count));
    for slot in 1..=SLOTS {
        prompt_inputs.insert(format!("character{slot}_enable"), json!(true));
        prompt_inputs.insert(format!("character{slot}_kind"), json!("character"));
    }
    for (index, string_id) in string_ids.iter().enumerate() {
        prompt_inputs.insert(format!("character{}", index + 1), json!([string_id, 0]));
    }
    let mut pipe = add_node(
        &mut workflow,
        &mut next_id,
        json!({ "class_type": "RSMultiCharacterPrompt", "inputs": prompt_inputs }),
    );

    if multi.characters.iter().any(|c| c.lora.is_some()) {
        let mut inputs = serde_json::Map::new();
        inputs.insert("building_pipe".into(), json!([pipe, 0]));
        inputs.insert("character_count".into(), json!(count));
        inputs.insert("lora_application_mode".into(), json!(multi.lora_mode));
        for slot in 1..=SLOTS {
            let lora = multi.characters.get(slot - 1).and_then(|c| c.lora.as_ref());
            inputs.insert(
                format!("character{slot}_lora"),
                json!(lora.map_or("None", |l| l.name.as_str())),
            );
            inputs.insert(
                format!("character{slot}_lora_strength"),
                json!(lora.map_or(1.0, |l| l.strength_model)),
            );
            inputs.insert(
                format!("character{slot}_lora_clip_strength"),
                json!(lora.map_or(1.0, |l| l.strength_clip)),
            );
        }
        pipe = add_node(
            &mut workflow,
            &mut next_id,
            json!({ "class_type": "RSMultiCharacterLoRA", "inputs": inputs }),
        );
    }

    let mut position_inputs = serde_json::Map::new();
    position_inputs.insert("building_pipe".into(), json!([pipe, 0]));
    position_inputs.insert("character_count".into(), json!(count));
    for slot in 1..=SLOTS {
        let (x, y, width, height) = if slot <= count {
            region_for(multi, slot - 1)
        } else {
            (0.5, 0.5, 0.4, 0.8)
        };
        position_inputs.insert(
            format!("character{slot}_geometry"),
            json!(if slot <= count { "box" } else { "point" }),
        );
        let (x, width) = axis_inside(x, width);
        let (y, height) = axis_inside(y, height);
        position_inputs.insert(format!("character{slot}_x"), json!(x));
        position_inputs.insert(format!("character{slot}_y"), json!(y));
        position_inputs.insert(format!("character{slot}_width"), json!(width));
        position_inputs.insert(format!("character{slot}_height"), json!(height));
    }
    pipe = add_node(
        &mut workflow,
        &mut next_id,
        json!({ "class_type": "RSMultiCharacterPosition", "inputs": position_inputs }),
    );

    let finalize_id = add_node(
        &mut workflow,
        &mut next_id,
        json!({
            "class_type": "RSMultiCharacterFinalize",
            "inputs": { "building_pipe": [pipe, 0] }
        }),
    );

    let (separation, gain) = cohesion_settings(multi);
    let (common_prior, common_boost) = scene_settings(multi);
    let sampler_id = add_node(
        &mut workflow,
        &mut next_id,
        json!({
            "class_type": "RSMultiCharacterSampler",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "clip": [clip_source.0.clone(), clip_source.1],
                "vae": [vae_source.0.clone(), vae_source.1],
                "character_pipe": [finalize_id, 0],
                "prompt": common_prompt,
                "negative_prompt": negative_prompt,
                "width": sampler_side(params.width),
                "height": sampler_side(params.height),
                "seed": seed,
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "prompt_syntax": "ComfyUI",
                "initial_spatial_separation": separation,
                "initial_character_gain": gain,
                "initial_common_prior": common_prior,
                "common_after_step3_boost": common_boost,
                // The app saves through MooshieSaveImage; the node's own save
                // would leave a second copy in ComfyUI's output folder.
                "autosave": false,
                "save_format": "PNG",
                "webp_quality": 90
            }
        }),
    );

    // The sampler hands back an image, not conditioning. The upscale, face fix
    // and segment passes that may follow need a positive and a negative, so give
    // them one built from the whole scene. Nodes nothing connects to never run.
    let mut scene = vec![common_prompt];
    scene.extend(character_prompts);
    scene.retain(|text| !text.is_empty());
    let positive_id = add_node(
        &mut workflow,
        &mut next_id,
        json!({
            "class_type": "CLIPTextEncode",
            "inputs": {
                "text": scene.join(", "),
                "clip": [clip_source.0.clone(), clip_source.1]
            }
        }),
    );
    let negative_id = add_node(
        &mut workflow,
        &mut next_id,
        json!({
            "class_type": "CLIPTextEncode",
            "inputs": {
                "text": negative_prompt,
                "clip": [clip_source.0.clone(), clip_source.1]
            }
        }),
    );

    WorkflowResult {
        workflow,
        next_id,
        image_output: (sampler_id.clone(), 0),
        model_source,
        clip_source,
        positive_source: (positive_id, 0),
        negative_source: (negative_id, 0),
        vae_source,
        sampler_id,
        refiner_model_source: None,
        base_sources: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comfyui::types::{LoraParam, MultiCharacter};

    fn lora(name: &str) -> LoraParam {
        LoraParam {
            name: name.to_string(),
            strength_model: 0.8,
            strength_clip: 0.6,
        }
    }

    fn params(loras: [Option<LoraParam>; 2]) -> GenerationParams {
        let [first, second] = loras;
        GenerationParams {
            mode: "txt2img".into(),
            model_architecture: "anima".into(),
            positive_prompt: "masterpiece, 2girls, cafe".into(),
            negative_prompt: "low quality".into(),
            checkpoint: "anima_base.safetensors".into(),
            sampler_name: "euler_ancestral".into(),
            scheduler: "sgm_uniform".into(),
            steps: 30,
            cfg: 4.0,
            width: 1344,
            height: 768,
            batch_size: 1,
            multi_character: Some(MultiCharacterParams {
                enabled: true,
                characters: vec![
                    MultiCharacter {
                        prompt: "1girl, blonde hair".into(),
                        lora: first,
                        region: None,
                    },
                    MultiCharacter {
                        prompt: "1girl, black hair".into(),
                        lora: second,
                        region: None,
                    },
                ],
                lora_mode: "precision".into(),
                cohesion: 0.0,
                scene_strength: 0.0,
                share_scene: false,
                say_position: false,
                interaction: String::new(),
                touch: false,
            }),
            ..Default::default()
        }
    }

    fn node<'a>(result: &'a WorkflowResult, class: &str) -> &'a Value {
        result
            .workflow
            .values()
            .find(|n| n["class_type"] == class)
            .unwrap_or_else(|| panic!("no {class} node"))
    }

    #[test]
    fn inactive_unless_enabled_anima_txt2img() {
        let mut p = params([None, None]);
        assert!(active(&p));
        p.multi_character.as_mut().unwrap().enabled = false;
        assert!(!active(&p));
        p.multi_character.as_mut().unwrap().enabled = true;
        p.model_architecture = "sdxl".into();
        assert!(!active(&p));
        p.model_architecture = "anima".into();
        p.mode = "img2img".into();
        assert!(!active(&p));
        p.mode = "txt2img".into();
        p.multi_character = None;
        assert!(!active(&p));
    }

    #[test]
    fn prompt_node_fills_all_ten_slots_and_connects_two_strings() {
        let result = build(&params([None, None]), 7);
        let inputs = &node(&result, "RSMultiCharacterPrompt")["inputs"];
        assert_eq!(inputs["character_count"], 2);
        for slot in 1..=SLOTS {
            assert_eq!(inputs[format!("character{slot}_enable")], true);
            assert_eq!(inputs[format!("character{slot}_kind")], "character");
        }
        assert!(inputs["character1"].is_array());
        assert!(inputs["character2"].is_array());
        assert!(inputs.get("character3").is_none());
        let texts: Vec<_> = result
            .workflow
            .values()
            .filter(|n| n["class_type"] == "PrimitiveStringMultiline")
            .map(|n| n["inputs"]["value"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(texts, ["1girl, blonde hair", "1girl, black hair"]);
    }

    #[test]
    fn lora_node_only_when_a_character_has_one() {
        let without = build(&params([None, None]), 1);
        assert!(!without
            .workflow
            .values()
            .any(|n| n["class_type"] == "RSMultiCharacterLoRA"));

        let with = build(&params([None, Some(lora("Chars/b.safetensors"))]), 1);
        let inputs = &node(&with, "RSMultiCharacterLoRA")["inputs"];
        assert_eq!(inputs["character1_lora"], "None");
        assert_eq!(inputs["character2_lora"], "Chars/b.safetensors");
        assert_eq!(inputs["character2_lora_strength"], 0.8);
        assert_eq!(inputs["character2_lora_clip_strength"], 0.6);
        assert_eq!(inputs["character3_lora"], "None");
        assert_eq!(inputs["lora_application_mode"], "precision");
    }

    #[test]
    fn characters_get_left_and_right_columns() {
        let result = build(&params([None, None]), 1);
        let inputs = &node(&result, "RSMultiCharacterPosition")["inputs"];
        assert_eq!(inputs["character1_geometry"], "box");
        assert_eq!(inputs["character1_x"], 0.25);
        assert_eq!(inputs["character2_x"], 0.75);
        assert_eq!(inputs["character1_width"], 0.5);
        assert_eq!(inputs["character1_height"], 1.0);
        assert_eq!(inputs["character3_geometry"], "point");
    }

    #[test]
    fn sampler_uses_the_main_prompt_and_run_settings() {
        let result = build(&params([None, None]), 99);
        let inputs = &node(&result, "RSMultiCharacterSampler")["inputs"];
        assert_eq!(inputs["prompt"], "masterpiece, 2girls, cafe");
        assert_eq!(inputs["negative_prompt"], "low quality");
        assert_eq!(inputs["seed"], 99);
        assert_eq!(inputs["steps"], 30);
        assert_eq!(inputs["width"], 1344);
        assert_eq!(inputs["autosave"], false);
        assert_eq!(result.image_output, (result.sampler_id.clone(), 0));
    }

    #[test]
    fn cohesion_moves_separation_and_gain_between_the_two_presets() {
        let mut p = params([None, None]);
        let multi = p.multi_character.as_mut().unwrap();
        assert_eq!(cohesion_settings(multi), (0.9, 1.7));
        multi.cohesion = 1.0;
        assert_eq!(cohesion_settings(multi), (0.5, 1.3));
        multi.cohesion = 0.5;
        assert_eq!(cohesion_settings(multi), (0.7, 1.5));
        multi.cohesion = 9.0;
        assert_eq!(cohesion_settings(multi), (0.5, 1.3));
        multi.cohesion = f64::NAN;
        assert_eq!(cohesion_settings(multi), (0.9, 1.7));
    }

    #[test]
    fn scene_strength_moves_the_common_prior_and_boost() {
        let mut p = params([None, None]);
        let multi = p.multi_character.as_mut().unwrap();
        assert_eq!(scene_settings(multi), (0.05, 0.1));
        multi.scene_strength = 1.0;
        assert_eq!(scene_settings(multi), (0.8, 0.43));
        multi.scene_strength = 0.6;
        assert_eq!(scene_settings(multi), (0.5, 0.3));
        multi.scene_strength = 9.0;
        assert_eq!(scene_settings(multi), (0.8, 0.43));
        multi.scene_strength = f64::NAN;
        assert_eq!(scene_settings(multi), (0.05, 0.1));

        multi.scene_strength = 0.6;
        let result = build(&p, 1);
        let sampler = node(&result, "RSMultiCharacterSampler");
        assert_eq!(sampler["inputs"]["initial_common_prior"], 0.5);
        assert_eq!(sampler["inputs"]["common_after_step3_boost"], 0.3);
    }

    #[test]
    fn shared_scene_is_appended_to_each_character_only_when_asked() {
        let mut p = params([None, None]);
        let texts = |p: &GenerationParams| -> Vec<String> {
            build(p, 1)
                .workflow
                .values()
                .filter(|n| n["class_type"] == "PrimitiveStringMultiline")
                .map(|n| n["inputs"]["value"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(texts(&p), ["1girl, blonde hair", "1girl, black hair"]);

        p.multi_character.as_mut().unwrap().share_scene = true;
        assert_eq!(
            texts(&p),
            [
                "1girl, blonde hair, masterpiece, 2girls, cafe",
                "1girl, black hair, masterpiece, 2girls, cafe"
            ]
        );
    }

    #[test]
    fn lora_tags_never_reach_a_character_prompt() {
        let mut p = params([None, None]);
        p.multi_character.as_mut().unwrap().characters[0].prompt =
            "1girl, <lora:other:1.0> blonde hair".into();
        let result = build(&p, 1);
        let texts: Vec<_> = result
            .workflow
            .values()
            .filter(|n| n["class_type"] == "PrimitiveStringMultiline")
            .map(|n| n["inputs"]["value"].as_str().unwrap().to_string())
            .collect();
        assert!(!texts[0].contains("lora"), "{texts:?}");
    }

    #[test]
    fn teacache_stays_out_of_the_graph() {
        let mut p = params([None, None]);
        p.anima_teacache_enabled = true;
        let result = build(&p, 1);
        assert!(!result
            .workflow
            .values()
            .any(|n| n["class_type"] == "MooshieAnimaTeaCache"));
    }

    #[test]
    fn refinement_passes_get_scene_conditioning() {
        let result = build(&params([None, None]), 1);
        let positive = &result.workflow[&result.positive_source.0]["inputs"]["text"];
        assert_eq!(
            positive,
            "masterpiece, 2girls, cafe, 1girl, blonde hair, 1girl, black hair"
        );
        let negative = &result.workflow[&result.negative_source.0]["inputs"]["text"];
        assert_eq!(negative, "low quality");
    }

    #[test]
    fn validation_accepts_a_plain_two_character_request() {
        assert_eq!(validate(&params([None, None])), Ok(()));
        assert_eq!(
            validate(&params([Some(lora("a.safetensors")), None])),
            Ok(())
        );
    }

    #[test]
    fn validation_is_a_no_op_when_disabled() {
        let mut p = params([None, None]);
        p.multi_character.as_mut().unwrap().enabled = false;
        p.batch_size = 4;
        assert_eq!(validate(&p), Ok(()));
    }

    #[test]
    fn validation_rejects_what_iteration_one_cannot_do() {
        let mut p = params([None, None]);
        p.batch_size = 2;
        assert!(validate(&p).unwrap_err().contains("batch size"));

        let mut p = params([None, None]);
        p.multi_character.as_mut().unwrap().characters.pop();
        assert!(validate(&p).unwrap_err().contains("between 2 and 6"));

        let mut p = params([None, None]);
        let extra = p.multi_character.as_ref().unwrap().characters[0].clone();
        p.multi_character.as_mut().unwrap().characters = vec![extra; 7];
        assert!(validate(&p).unwrap_err().contains("between 2 and 6"));

        let mut p = params([None, None]);
        p.multi_character.as_mut().unwrap().characters[1].prompt = "  ".into();
        assert!(validate(&p).unwrap_err().contains("Character 2"));

        let mut p = params([None, Some(lora(" "))]);
        assert!(validate(&p).unwrap_err().contains("LoRA without a file"));
        p.multi_character.as_mut().unwrap().characters[1].lora = None;
        p.multi_character.as_mut().unwrap().lora_mode = "turbo".into();
        assert!(validate(&p).unwrap_err().contains("LoRA mode"));

        let mut p = params([None, None]);
        p.style_ref_enabled = true;
        assert!(validate(&p).unwrap_err().contains("reference"));

        let mut p = params([None, None]);
        p.pause_at_step = Some(5);
        assert!(validate(&p).unwrap_err().contains("pause"));

        let mut p = params([None, None]);
        p.model_architecture = "sdxl".into();
        assert!(validate(&p)
            .unwrap_err()
            .contains("only available for Anima"));
    }

    fn set_region(p: &mut GenerationParams, index: usize, x: f64, y: f64, w: f64, h: f64) {
        p.multi_character.as_mut().unwrap().characters[index].region =
            Some(crate::comfyui::types::CharacterRegion {
                x,
                y,
                width: w,
                height: h,
            });
    }

    #[test]
    fn own_boxes_replace_the_columns() {
        let mut p = params([None, None]);
        set_region(&mut p, 0, 0.3, 0.6, 0.5, 0.8);
        set_region(&mut p, 1, 0.75, 0.5, 0.4, 1.0);
        assert!(validate(&p).is_ok());
        let result = build(&p, 1);
        let inputs = &node(&result, "RSMultiCharacterPosition")["inputs"];
        assert_eq!(inputs["character1_x"], 0.3);
        assert_eq!(inputs["character1_y"], 0.6);
        assert_eq!(inputs["character1_width"], 0.5);
        assert_eq!(inputs["character1_height"], 0.8);
        assert_eq!(inputs["character2_x"], 0.75);
        assert_eq!(inputs["character2_width"], 0.4);
    }

    #[test]
    fn a_character_without_a_box_keeps_its_column() {
        let mut p = params([None, None]);
        set_region(&mut p, 1, 0.7, 0.5, 0.5, 1.0);
        let result = build(&p, 1);
        let inputs = &node(&result, "RSMultiCharacterPosition")["inputs"];
        assert_eq!(inputs["character1_x"], 0.25);
        assert_eq!(inputs["character1_width"], 0.5);
        assert_eq!(inputs["character2_x"], 0.7);
    }

    #[test]
    fn boxes_that_leave_the_image_or_are_tiny_are_refused() {
        let mut p = params([None, None]);
        set_region(&mut p, 0, 0.95, 0.5, 0.4, 1.0);
        assert!(validate(&p).is_err());
        set_region(&mut p, 0, 0.5, 0.5, 0.05, 1.0);
        assert!(validate(&p).is_err());
        set_region(&mut p, 0, f64::NAN, 0.5, 0.5, 1.0);
        assert!(validate(&p).is_err());
    }

    #[test]
    fn saying_the_position_adds_words_only_when_asked() {
        let mut p = params([None, None]);
        let texts = |result: &WorkflowResult| -> Vec<String> {
            result
                .workflow
                .values()
                .filter(|n| n["class_type"] == "PrimitiveStringMultiline")
                .map(|n| n["inputs"]["value"].as_str().unwrap().to_string())
                .collect()
        };
        assert!(!texts(&build(&p, 1)).iter().any(|t| t.contains("side")));
        p.multi_character.as_mut().unwrap().say_position = true;
        let said = texts(&build(&p, 1));
        assert!(said.contains(&"1girl, blonde hair, left side".to_string()));
        assert!(said.contains(&"1girl, black hair, right side".to_string()));
        set_region(&mut p, 0, 0.5, 0.5, 0.5, 1.0);
        assert!(texts(&build(&p, 1)).contains(&"1girl, blonde hair".to_string()));
    }

    #[test]
    fn interaction_text_goes_to_every_character_and_overlaps_the_boxes() {
        let mut p = params([None, None]);
        p.multi_character.as_mut().unwrap().interaction = "hugging each other".into();
        let result = build(&p, 1);
        let texts: Vec<String> = result
            .workflow
            .values()
            .filter(|n| n["class_type"] == "PrimitiveStringMultiline")
            .map(|n| n["inputs"]["value"].as_str().unwrap().to_string())
            .collect();
        assert!(texts.contains(&"1girl, blonde hair, hugging each other".to_string()));
        assert!(texts.contains(&"1girl, black hair, hugging each other".to_string()));
        let inputs = &node(&result, "RSMultiCharacterPosition")["inputs"];
        assert_eq!(inputs["character1_x"], 0.35);
        assert_eq!(inputs["character2_x"], 0.65);
        assert_eq!(inputs["character1_width"], 0.65);
    }

    #[test]
    fn own_boxes_win_over_the_interaction_layout() {
        let mut p = params([None, None]);
        p.multi_character.as_mut().unwrap().interaction = "holding hands".into();
        set_region(&mut p, 0, 0.3, 0.5, 0.5, 1.0);
        let result = build(&p, 1);
        let inputs = &node(&result, "RSMultiCharacterPosition")["inputs"];
        assert_eq!(inputs["character1_x"], 0.3);
        assert_eq!(inputs["character2_x"], 0.65);
    }

    #[test]
    fn three_to_six_characters_fill_every_slot_in_order() {
        for count in 3..=MAX_CHARACTERS {
            let mut p = params([Some(lora("a.safetensors")), None]);
            let template = p.multi_character.as_ref().unwrap().characters[1].clone();
            p.multi_character
                .as_mut()
                .unwrap()
                .characters
                .resize(count, template);
            for (i, c) in p
                .multi_character
                .as_mut()
                .unwrap()
                .characters
                .iter_mut()
                .enumerate()
            {
                c.prompt = format!("char{i}");
            }
            assert_eq!(validate(&p), Ok(()));
            let result = build(&p, 1);
            let prompt = node(&result, "RSMultiCharacterPrompt");
            assert_eq!(prompt["inputs"]["character_count"], count);
            let position = &node(&result, "RSMultiCharacterPosition")["inputs"];
            assert_eq!(position["character_count"], count);
            for slot in 1..=count {
                assert_eq!(position[format!("character{slot}_geometry")], "box");
            }
            // Columns tile the width without a gap.
            let width = position["character1_width"].as_f64().unwrap();
            assert!((width - round4(1.0 / count as f64)).abs() < 1e-3);
        }
    }

    #[test]
    fn overlapping_boxes_for_many_characters_stay_inside_the_image() {
        for count in 2..=MAX_CHARACTERS {
            for index in 0..count {
                let (x, y, w, h) = overlapping_box(index, count);
                assert!(
                    x - w / 2.0 >= -1e-9 && x + w / 2.0 <= 1.0 + 1e-9,
                    "{count}/{index}"
                );
                assert!(w >= MIN_REGION_SIDE && y == 0.5 && h == 1.0);
            }
        }
        // Neighbours overlap.
        let (a, _, aw, _) = overlapping_box(0, 4);
        let (b, _, _, _) = overlapping_box(1, 4);
        assert!(b - a < aw);
    }

    #[test]
    fn rounded_boxes_never_leave_the_image() {
        // Four columns put the last box at 0.875 +/- 0.125; two-decimal rounding of the
        // centre alone gave an edge of 1.005, which the sampler refuses.
        for count in 2..=MAX_CHARACTERS {
            let mut p = params([None, None]);
            let template = p.multi_character.as_ref().unwrap().characters[0].clone();
            p.multi_character
                .as_mut()
                .unwrap()
                .characters
                .resize(count, template);
            let position = build(&p, 1);
            let inputs = &node(&position, "RSMultiCharacterPosition")["inputs"];
            for slot in 1..=count {
                for (c, s) in [("x", "width"), ("y", "height")] {
                    let centre = inputs[format!("character{slot}_{c}")].as_f64().unwrap();
                    let size = inputs[format!("character{slot}_{s}")].as_f64().unwrap();
                    assert!(centre - size / 2.0 >= -1e-9, "{count}/{slot}");
                    assert!(centre + size / 2.0 <= 1.0 + 1e-9, "{count}/{slot}");
                }
            }
        }
        // A box dragged without snapping, centre 0.9625, width 0.075.
        let (x, w) = axis_inside(0.9625, 0.075);
        assert!(x + w / 2.0 <= 1.0 + 1e-9);
    }

    #[test]
    fn sides_snap_to_multiples_of_sixteen_for_the_sampler() {
        assert_eq!(sampler_side(1368), 1376);
        assert_eq!(sampler_side(1024), 1024);
        assert_eq!(sampler_side(1360), 1360);
        assert_eq!(sampler_side(1352), 1360);
        assert_eq!(sampler_side(8), 64);
        assert_eq!(sampler_side(5000), 4096);
        let mut p = params([None, None]);
        p.width = 1368;
        let result = build(&p, 1);
        assert_eq!(
            node(&result, "RSMultiCharacterSampler")["inputs"]["width"],
            1376
        );
    }

    #[test]
    fn touching_overlaps_the_boxes_without_interaction_text() {
        let mut p = params([None, None]);
        let position = |p: &GenerationParams| {
            let r = build(p, 1);
            node(&r, "RSMultiCharacterPosition")["inputs"].clone()
        };
        assert_eq!(position(&p)["character1_width"], 0.5);
        p.multi_character.as_mut().unwrap().touch = true;
        let touching = position(&p);
        assert_eq!(touching["character1_width"], 0.65);
        assert_eq!(touching["character1_x"], 0.35);
        // The prompts are untouched: no interaction text was set.
        let r = build(&p, 1);
        let prompts: Vec<String> = r
            .workflow
            .values()
            .filter(|n| n["class_type"] == "PrimitiveStringMultiline")
            .map(|n| n["inputs"]["value"].as_str().unwrap().to_string())
            .collect();
        assert!(prompts.iter().all(|t| !t.contains("hugging")));
    }
}
