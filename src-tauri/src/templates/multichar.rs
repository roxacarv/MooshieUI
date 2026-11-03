//! Anima multi-character workflow.
//!
//! Each character gets its own prompt, an optional LoRA and a slice of the
//! canvas, and the RS MultiCharacter Sampler custom node pack (pinned in
//! `comfyui/nodes.rs`) routes them so attributes and LoRAs stay with the
//! character they belong to. Iteration one: exactly two characters side by side,
//! no interaction between them.

use serde_json::{json, Value};

use super::{load_model_nodes, strip_lora_tags, WorkflowResult};
use crate::comfyui::types::{GenerationParams, MultiCharacterParams};

/// Every RS builder node requires all ten slots, whether they are used or not.
const SLOTS: usize = 10;

/// Characters the first iteration supports.
pub const CHARACTER_COUNT: usize = 2;

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
    if multi.characters.len() != CHARACTER_COUNT {
        return Err(format!(
            "Multi-character generation needs exactly {CHARACTER_COUNT} characters."
        ));
    }
    for (index, character) in multi.characters.iter().enumerate() {
        if character.prompt.trim().is_empty() {
            return Err(format!("Character {} needs a description.", index + 1));
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

/// Sampler defaults of `initial_common_prior` / `common_after_step3_boost`
/// (strength 0) and the values they reach at strength 1. The node keeps the
/// scene prompt at the prior for the first 40% of denoising, so with the
/// defaults a background asked for in the main prompt never shows up.
const COMMON_PRIOR_MIN: f64 = 0.05;
const COMMON_PRIOR_MAX: f64 = 0.8;
const COMMON_BOOST_MIN: f64 = 0.1;
const COMMON_BOOST_MAX: f64 = 0.43;

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
    let column_prompts: Vec<String> = character_prompts
        .iter()
        .map(|text| {
            if multi.share_scene && !common_prompt.is_empty() {
                format!("{text}, {common_prompt}")
            } else {
                text.clone()
            }
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
            column(slot - 1, count)
        } else {
            (0.5, 0.5, 0.4, 0.8)
        };
        position_inputs.insert(
            format!("character{slot}_geometry"),
            json!(if slot <= count { "box" } else { "point" }),
        );
        position_inputs.insert(format!("character{slot}_x"), json!(round2(x)));
        position_inputs.insert(format!("character{slot}_y"), json!(round2(y)));
        position_inputs.insert(format!("character{slot}_width"), json!(round2(width)));
        position_inputs.insert(format!("character{slot}_height"), json!(round2(height)));
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
                "width": params.width,
                "height": params.height,
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
                    },
                    MultiCharacter {
                        prompt: "1girl, black hair".into(),
                        lora: second,
                    },
                ],
                lora_mode: "precision".into(),
                cohesion: 0.0,
                scene_strength: 0.0,
                share_scene: false,
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
        assert!(validate(&p).unwrap_err().contains("exactly 2"));

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
}
