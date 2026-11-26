//! The image prompt: fixed blocks put together by code, and a person's note worked in by an LLM
//! without touching the locked sentences.
//!
//! - **Assembly** ([`assemble`]): the pack's blocks in its order. A block is a list of sentences,
//!   each with an id (`STYLE.color`), a lock flag and optional `when` conditions (kind, style
//!   references, outline mode, background). Placeholders (`{pitch}`, `{yaw}`, `{side}`,
//!   `{keyColor}`, ...) come from the capture values and options. Same inputs, same bytes.
//! - **Fidelity** (`when.fidelity`): `strict` repaints the capture and keeps every part (count,
//!   position, relative size, pattern) and adds nothing; `stylized` may exaggerate, simplify and add
//!   small charms. Things a note asks to add are listed after the "add nothing" rule as exceptions.
//! - **Empty note** (`trim()` is empty): the fixed text is what is sent. No LLM is called.
//! - **Note** ([`rewrite_prompt`], [`apply_edits`]): the LLM returns an edit list (replace an
//!   unlocked sentence, add a sentence to an addable block, things to avoid, rejected parts), not a
//!   new prompt. Code checks it, puts the prompt together again and makes sure every locked
//!   sentence is byte for byte the fixed one. Edits that touch locked sentences, unknown ids, banned
//!   terms or things the output rules forbid (background, text, logos ...) are moved to `rejected`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::packs::Pack;

pub const REWRITE_SCHEMA: &str = include_str!("../resources/schemas/rewrite.schema.json");
pub const REVIEW_SCHEMA: &str = include_str!("../resources/schemas/review.schema.json");
pub const FIDELITY_SCHEMA: &str = include_str!("../resources/schemas/fidelity.schema.json");
pub const FIDELITIES: &[&str] = &["strict", "stylized"];

pub const MAX_REPLACE: usize = 6;
pub const MAX_ADD: usize = 4;
pub const MAX_AVOID: usize = 4;
pub const MAX_SENTENCE: usize = 240;
/// Share of letters outside Latin scripts above which a sentence is not English.
const MAX_NON_LATIN: f64 = 0.10;

/// What the prompt is put together for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PromptContext {
    pub kind: String,
    /// Camera turn in degrees (sign gives the side shown) and look-down angle
    pub yaw: f64,
    pub pitch: f64,
    /// Number of style reference pictures sent after the capture
    pub refs: usize,
    /// `code` (drawn by the tool after), `model`, `none`
    pub outline: String,
    /// `transparent` or `flat`
    pub background: String,
    /// The flat background color (`#RRGGBB`), when `background` is `flat`
    pub key_color: String,
    /// `strict` (keep the capture, add nothing) or `stylized`
    pub fidelity: String,
    /// `orthographic` or `perspective` (with `fov` degrees), as captured
    pub projection: String,
    pub fov: f64,
}

impl Default for PromptContext {
    fn default() -> Self {
        PromptContext {
            kind: "generic".into(),
            yaw: 28.0,
            pitch: 30.0,
            refs: 0,
            outline: "code".into(),
            background: "transparent".into(),
            key_color: "#FF00FF".into(),
            fidelity: "strict".into(),
            projection: "orthographic".into(),
            fov: 20.0,
        }
    }
}

impl PromptContext {
    /// Context for `kind` with the pack's capture angles for it.
    pub fn for_kind(pack: &Pack, kind: &str) -> PromptContext {
        let preset = pack.kind_preset(kind);
        let cap = preset.get("capture");
        let num = |k: &str, d: f64| cap.and_then(|c| c.get(k)).and_then(Value::as_f64).unwrap_or(d);
        let projection = cap.and_then(|c| c.get("projection")).and_then(Value::as_str).unwrap_or("orthographic").to_string();
        PromptContext { kind: kind.into(), yaw: num("yaw", 28.0), pitch: num("pitch", 30.0), projection, fov: num("fov", 20.0), ..PromptContext::default() }
    }

    /// Takes the camera of a capture's values (`yaw`, `pitch`, `projection`, `fov`), so the
    /// CAMERA sentence says what the picture shows.
    pub fn with_capture(mut self, values: &serde_json::Map<String, Value>) -> PromptContext {
        if let Some(y) = values.get("yaw").and_then(Value::as_f64) {
            self.yaw = y;
        }
        if let Some(p) = values.get("pitch").and_then(Value::as_f64) {
            self.pitch = p;
        }
        if let Some(p) = values.get("projection").and_then(Value::as_str) {
            self.projection = p.to_string();
        }
        if let Some(f) = values.get("fov").and_then(Value::as_f64) {
            self.fov = f;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Same,
    Replaced,
    Added,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sentence {
    /// `BLOCK.id`
    pub id: String,
    pub block: String,
    pub locked: bool,
    pub text: String,
    pub status: Status,
    /// The fixed text, for a replaced sentence
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assembled {
    pub pack_id: String,
    pub template_version: String,
    pub context: PromptContext,
    pub sentences: Vec<Sentence>,
    /// Blocks in order, sentences joined by a space, blocks by a blank line
    pub text: String,
    /// Blocks an LLM may add sentences to
    pub addable: Vec<String>,
}

fn number(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{}", (v * 100.0).round() / 100.0)
    }
}

fn when_matches(when: Option<&Value>, ctx: &PromptContext) -> bool {
    let Some(Value::Object(w)) = when else { return true };
    let one_of = |v: &Value, actual: &str| match v {
        Value::String(s) => s == actual,
        Value::Array(a) => a.iter().any(|x| x.as_str() == Some(actual)),
        _ => false,
    };
    w.iter().all(|(k, v)| match k.as_str() {
        "refs" => v.as_bool().is_some_and(|b| b == (ctx.refs > 0)),
        "kind" => one_of(v, &ctx.kind),
        "outline" => one_of(v, &ctx.outline),
        "background" => one_of(v, &ctx.background),
        "fidelity" => one_of(v, &ctx.fidelity),
        _ => false,
    })
}

fn fill(text: &str, ctx: &PromptContext, pack: &Pack) -> String {
    let outline = pack.doc.get("outline");
    let canvas = pack.doc.get("canvas");
    let side = if ctx.yaw < 0.0 { "left" } else { "right" };
    let pairs = [
        ("{pitch}", number(ctx.pitch)),
        ("{yaw}", number(ctx.yaw.abs())),
        ("{side}", side.to_string()),
        (
            "{projection}",
            if ctx.projection == "perspective" {
                format!("a mild perspective like the capture (about a {} degree field of view)", number(ctx.fov))
            } else {
                "orthographic-like projection with parallel vertical edges, no wide-angle distortion".to_string()
            },
        ),
        ("{keyColor}", ctx.key_color.clone()),
        ("{lastRef}", (ctx.refs + 1).to_string()),
        (
            "{refImages}",
            if ctx.refs == 1 { "Input image 2 is a style reference".to_string() } else { format!("Input images 2 to {} are style references", ctx.refs + 1) },
        ),
        ("{outlineColor}", outline.and_then(|o| o.get("color")).and_then(Value::as_str).unwrap_or("#753719").to_string()),
        ("{outlineWidthPct}", number(outline.and_then(|o| o.get("widthPct")).and_then(Value::as_f64).unwrap_or(1.45))),
        ("{fillPct}", number(canvas.and_then(|c| c.get("fillPct")).and_then(Value::as_f64).unwrap_or(88.0))),
    ];
    let mut out = text.to_string();
    for (k, v) in pairs {
        out = out.replace(k, &v);
    }
    out
}

pub fn join(sentences: &[Sentence], order: &[String]) -> String {
    let mut blocks: Vec<String> = Vec::new();
    for b in order {
        let parts: Vec<&str> = sentences.iter().filter(|s| &s.block == b).map(|s| s.text.as_str()).collect();
        if !parts.is_empty() {
            blocks.push(parts.join(" "));
        }
    }
    blocks.join("\n\n")
}

pub fn order_of(pack: &Pack) -> Vec<String> {
    pack.doc
        .pointer("/prompt/order")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// The fixed prompt for `ctx` from the pack's blocks.
pub fn assemble(pack: &Pack, ctx: &PromptContext) -> Result<Assembled, String> {
    let prompt = pack.doc.get("prompt").ok_or("pack:noPrompt")?;
    let order = order_of(pack);
    let blocks = prompt.get("blocks").and_then(Value::as_object).ok_or("pack:noBlocks")?;
    let mut sentences = Vec::new();
    for b in &order {
        let Some(list) = blocks.get(b).and_then(Value::as_array) else { continue };
        for s in list {
            if !when_matches(s.get("when"), ctx) {
                continue;
            }
            let (Some(id), Some(text)) = (s.get("id").and_then(Value::as_str), s.get("text").and_then(Value::as_str)) else {
                continue;
            };
            let full = format!("{b}.{id}");
            if sentences.iter().any(|x: &Sentence| x.id == full) {
                return Err(format!("pack:duplicateSentence:{full}"));
            }
            sentences.push(Sentence {
                id: full,
                block: b.clone(),
                locked: s.get("locked").and_then(Value::as_bool).unwrap_or(true),
                text: fill(text, ctx, pack),
                status: Status::Same,
                before: None,
            });
        }
    }
    if sentences.is_empty() {
        return Err("pack:emptyPrompt".into());
    }
    let addable = prompt
        .get("addable")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_else(|| vec!["STYLE".into(), "SUBJECT".into()]);
    let text = join(&sentences, &order);
    Ok(Assembled {
        pack_id: pack.id.clone(),
        template_version: prompt.get("templateVersion").and_then(Value::as_str).unwrap_or("0").to_string(),
        context: ctx.clone(),
        sentences,
        text,
        addable,
    })
}

/// True when the note counts as empty (only spaces and line breaks).
pub fn note_is_empty(note: &str) -> bool {
    note.trim().is_empty()
}

/// The outer request to Codex: use the image tool once with the prompt as written.
pub fn generator_text(pack: &Pack, background: &str, prompt: &str) -> String {
    let outer = pack
        .doc
        .pointer(&format!("/prompt/generator/{background}"))
        .and_then(Value::as_str)
        .unwrap_or("Use your built-in image generation tool exactly once. Edit input image 1 as described in PROMPT; pass PROMPT to the tool as written. After saving, reply with only the absolute path of the saved image file.");
    format!("{outer}\nPROMPT: <<<\n{prompt}\n>>>\n")
}

// ------------------------------------------------------------------ rewrite request

/// The instruction for the LLM that turns a note into edits (English; the note may be any language).
pub fn rewrite_prompt(fixed: &Assembled, note: &str, retry_reason: Option<&str>) -> String {
    let sentences: Vec<Value> = fixed.sentences.iter().map(|s| json!({ "id": s.id, "locked": s.locked, "text": s.text })).collect();
    let input = json!({
        "kind": fixed.context.kind,
        "hasStyleRefs": fixed.context.refs > 0,
        "fidelity": fixed.context.fidelity,
        "sentences": sentences,
        "addableBlocks": fixed.addable,
        "note": note.trim(),
    });
    let mut out = String::from(
        "You edit an image-generation prompt for a game-icon tool. The prompt is a list of sentences with ids. \
Work the user's note into it and answer with an edit list as JSON only (no prose).\n\n\
RULES (highest priority first)\n\
1. Sentences with \"locked\": true never change. Never put their ids in \"replace\".\n\
   They fix what the subject is (shape, parts, viewpoint); a note may still change colors, materials, mood or \
small details. Apply such a change in an unlocked sentence or one added SUBJECT sentence, naming exactly what \
changes; it wins over the capture for that part only.\n\
2. Things the output rules forbid are not applied: a background, scenery, a floor, text, letters, numbers, logos, \
watermarks, frames, borders, badges, several images or views. List each such part of the note in \"rejected\" with a \
short Korean reason.\n\
3. Translate the note into concise English. For each requested change, replace the unlocked sentence it belongs to \
(give the whole new sentence: keep its unchanged parts word for word and its leading label such as \"Color:\"), or, \
when no sentence fits, add one short sentence to one of addableBlocks. Never add a sentence that contradicts another one.\n\
4. Apply only what the note asks. Add no style ideas of your own. Never write game titles, brand names or character names.\n\
5. \"outline\": \"off\" only when the note asks for no outline, else \"keep\". \"camera\": \"front\" when it asks for a \
straight front view, \"flip\" when it asks to show the other side, else \"keep\". These sentences are not edited.\n\
6. \"avoidAdd\": short English noun phrases for things the note wants kept out of the picture.\n\
7. \"summaryKo\": one short Korean sentence on what was applied.\n\
8. \"additions\": short English noun phrases for new things the note explicitly asks to put on or around the object \
(for example \"snow on the roof\"); empty when it asks for none. Describe each one also in an added SUBJECT sentence. \
With \"fidelity\": \"strict\" nothing else may be added, so list every requested addition here.\n\
9. At most 6 replace, 4 add, 4 avoidAdd and 4 additions items; each text at most 240 characters; all texts in English except the \
Korean reasons and summaryKo.\n\n\
INPUT\n",
    );
    out.push_str(&serde_json::to_string_pretty(&input).unwrap_or_default());
    out.push('\n');
    if let Some(why) = retry_reason {
        out.push_str(&format!(
            "\nYour previous answer could not be used ({why}). Answer again following the rules, JSON only.\n"
        ));
    }
    out
}

// ------------------------------------------------------------------ edits from the LLM

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Edits {
    pub replace: Vec<ReplaceEdit>,
    pub add: Vec<AddEdit>,
    pub avoid_add: Vec<String>,
    /// Things the note asks to add (exceptions to the strict "add nothing" rule)
    pub additions: Vec<String>,
    pub outline: String,
    pub camera: String,
    pub rejected: Vec<Rejected>,
    pub summary_ko: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplaceEdit {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AddEdit {
    pub block: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rejected {
    pub part: String,
    pub reason: String,
    /// `model` (the LLM left it out) or `code` (the tool took it out)
    pub by: String,
}

/// Parses the LLM's answer. `Err` names the problem (sent back on the one retry).
pub fn parse_edits(value: &Value) -> Result<Edits, String> {
    if !value.is_object() {
        return Err("not a JSON object".into());
    }
    let edits: Edits = serde_json::from_value(value.clone()).map_err(|e| format!("does not match the schema: {e}"))?;
    if edits.replace.len() > MAX_REPLACE || edits.add.len() > MAX_ADD || edits.avoid_add.len() > MAX_AVOID || edits.additions.len() > MAX_ADD {
        return Err(format!("too many items (replace <= {MAX_REPLACE}, add <= {MAX_ADD}, avoidAdd <= {MAX_AVOID})"));
    }
    let texts = edits.replace.iter().map(|r| &r.text).chain(edits.add.iter().map(|a| &a.text)).chain(edits.avoid_add.iter()).chain(edits.additions.iter());
    for t in texts {
        if t.trim().is_empty() {
            return Err("an empty text".into());
        }
        if t.chars().count() > MAX_SENTENCE {
            return Err(format!("a text longer than {MAX_SENTENCE} characters"));
        }
        if !is_english(t) {
            return Err(format!("a text that is not in English: {:?}", t.chars().take(40).collect::<String>()));
        }
    }
    Ok(edits)
}

/// True unless more than 10% of the letters are outside Latin scripts.
pub fn is_english(text: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return true;
    }
    let non_latin = letters.iter().filter(|c| (**c as u32) > 0x024F).count();
    (non_latin as f64) / (letters.len() as f64) <= MAX_NON_LATIN
}

/// Words that would put a background, text or a frame in the picture: an edit with one of them is
/// taken out (the output rules are locked).
const FORBIDDEN: &[&str] = &[
    "background", "backdrop", "scenery", "landscape", "text", "letter", "letters", "lettering", "caption", "title",
    "label", "logo", "logos", "watermark", "signature", "frame", "border", "badge", "numbers",
];

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// The first forbidden word in `text`, if any.
pub fn forbidden_word(text: &str) -> Option<String> {
    let ws = words(text);
    FORBIDDEN.iter().find(|f| ws.iter().any(|w| w == *f)).map(|f| f.to_string())
}

/// The first banned term of the pack found in `text` (case-insensitive, as a substring).
pub fn banned_term(pack: &Pack, text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    pack.doc
        .get("bannedTerms")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .find(|t| lower.contains(&t.to_lowercase()))
        .map(str::to_string)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rewritten {
    pub sentences: Vec<Sentence>,
    pub text: String,
    pub rejected: Vec<Rejected>,
    pub summary_ko: String,
    /// `keep` or `off` (off needs the person's yes; it changes only the drawing after generation)
    pub outline: String,
    /// `keep`, `front` or `flip`: a suggestion to capture again, never applied to the prompt
    pub camera: String,
    pub changed: bool,
    /// Things the note asks to add; the fidelity review does not count them as additions
    #[serde(default)]
    pub additions: Vec<String>,
}

/// Applies checked edits to the fixed prompt. Locked sentences stay byte for byte (checked).
pub fn apply_edits(pack: &Pack, fixed: &Assembled, edits: &Edits) -> Result<Rewritten, String> {
    let mut sentences = fixed.sentences.clone();
    let mut rejected: Vec<Rejected> = edits
        .rejected
        .iter()
        .filter(|r| !r.part.trim().is_empty())
        .map(|r| Rejected { part: r.part.clone(), reason: r.reason.clone(), by: "model".into() })
        .collect();
    let code_reject = |part: &str, reason: String| Rejected { part: part.into(), reason, by: "code".into() };
    let screen = |text: &str| -> Option<String> {
        if let Some(t) = banned_term(pack, text) {
            return Some(format!("banned:{t}"));
        }
        forbidden_word(text).map(|w| format!("forbidden:{w}"))
    };

    for r in &edits.replace {
        let Some(s) = sentences.iter_mut().find(|s| s.id == r.id) else {
            rejected.push(code_reject(&r.text, format!("unknownId:{}", r.id)));
            continue;
        };
        if s.locked {
            rejected.push(code_reject(&r.text, format!("locked:{}", r.id)));
            continue;
        }
        if let Some(why) = screen(&r.text) {
            rejected.push(code_reject(&r.text, why));
            continue;
        }
        let new_text = r.text.trim().to_string();
        if new_text == s.text {
            continue;
        }
        if s.status == Status::Same {
            s.before = Some(s.text.clone());
        }
        s.text = new_text;
        s.status = Status::Replaced;
    }

    let mut added = 0usize;
    for a in &edits.add {
        if !fixed.addable.iter().any(|b| b == &a.block) {
            rejected.push(code_reject(&a.text, format!("notAddable:{}", a.block)));
            continue;
        }
        if let Some(why) = screen(&a.text) {
            rejected.push(code_reject(&a.text, why));
            continue;
        }
        added += 1;
        let sentence = Sentence {
            id: format!("{}.add{added}", a.block),
            block: a.block.clone(),
            locked: false,
            text: a.text.trim().to_string(),
            status: Status::Added,
            before: None,
        };
        let at = sentences.iter().rposition(|s| s.block == a.block).map(|i| i + 1).unwrap_or(sentences.len());
        sentences.insert(at, sentence);
    }

    let mut avoid: Vec<String> = Vec::new();
    for item in &edits.avoid_add {
        let item = item.trim().trim_end_matches('.').to_string();
        if let Some(t) = banned_term(pack, &item) {
            rejected.push(code_reject(&item, format!("banned:{t}")));
            continue;
        }
        if !avoid.contains(&item) {
            avoid.push(item);
        }
    }
    if !avoid.is_empty() {
        let sentence = Sentence {
            id: "AVOID.add".into(),
            block: "AVOID".into(),
            locked: false,
            text: format!("Also avoid: {}.", avoid.join(", ")),
            status: Status::Added,
            before: None,
        };
        let at = sentences.iter().rposition(|s| s.block == "AVOID").map(|i| i + 1).unwrap_or(sentences.len());
        sentences.insert(at, sentence);
    }

    let mut additions: Vec<String> = Vec::new();
    for item in &edits.additions {
        let item = item.trim().trim_end_matches('.').to_string();
        if item.is_empty() {
            continue;
        }
        if let Some(why) = screen(&item) {
            rejected.push(code_reject(&item, why));
            continue;
        }
        if !additions.contains(&item) {
            additions.push(item);
        }
    }
    if !additions.is_empty() && fixed.context.fidelity == "strict" {
        // Right after the locked "add nothing" rule (or at the end of AVOID)
        let sentence = Sentence {
            id: "AVOID.allow".into(),
            block: "AVOID".into(),
            locked: false,
            text: format!("Except what the note asks for: {}.", additions.join(", ")),
            status: Status::Added,
            before: None,
        };
        let at = sentences
            .iter()
            .position(|s| s.id == "AVOID.additions")
            .map(|i| i + 1)
            .or_else(|| sentences.iter().rposition(|s| s.block == "AVOID").map(|i| i + 1))
            .unwrap_or(sentences.len());
        sentences.insert(at, sentence);
    }

    check_locked(fixed, &sentences)?;
    let order = order_of(pack);
    let text = join(&sentences, &order);
    let outline = if edits.outline == "off" { "off" } else { "keep" };
    let camera = match edits.camera.as_str() {
        "front" | "flip" => edits.camera.clone(),
        _ => "keep".into(),
    };
    Ok(Rewritten {
        changed: text != fixed.text,
        sentences,
        text,
        rejected,
        summary_ko: edits.summary_ko.trim().to_string(),
        outline: outline.into(),
        camera,
        additions,
    })
}

/// `sentences` with one more unlocked sentence at the end of `block`, and the joined text.
pub fn with_sentence(pack: &Pack, sentences: &[Sentence], block: &str, id: &str, text: &str) -> (Vec<Sentence>, String) {
    let mut out = sentences.to_vec();
    let s = Sentence { id: format!("{block}.{id}"), block: block.into(), locked: false, text: text.into(), status: Status::Added, before: None };
    let at = out.iter().rposition(|x| x.block == block).map(|i| i + 1).unwrap_or(out.len());
    out.insert(at, s);
    let text = join(&out, &order_of(pack));
    (out, text)
}

/// Every locked sentence of `fixed` is in `sentences`, same text, same order.
pub fn check_locked(fixed: &Assembled, sentences: &[Sentence]) -> Result<(), String> {
    let want: Vec<(&str, &str)> = fixed.sentences.iter().filter(|s| s.locked).map(|s| (s.id.as_str(), s.text.as_str())).collect();
    let got: Vec<(&str, &str)> = sentences.iter().filter(|s| s.locked).map(|s| (s.id.as_str(), s.text.as_str())).collect();
    if want != got {
        let id = want.iter().zip(got.iter()).find(|(a, b)| a != b).map(|(a, _)| a.0).unwrap_or("?");
        return Err(format!("lockedChanged:{id}"));
    }
    Ok(())
}

/// The before/after view kept as `diff.json`.
pub fn diff(fixed: &Assembled, rewritten: Option<&Rewritten>) -> Value {
    match rewritten {
        None => json!({
            "changed": false,
            "sentences": fixed.sentences,
            "rejected": [],
            "summaryKo": "",
            "outline": "keep",
            "camera": "keep",
        }),
        Some(r) => json!({
            "changed": r.changed,
            "sentences": r.sentences,
            "rejected": r.rejected,
            "summaryKo": r.summary_ko,
            "outline": r.outline,
            "camera": r.camera,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn building() -> (Pack, Assembled) {
        let pack = Pack::builtin();
        let ctx = PromptContext::for_kind(&pack, "building");
        let a = assemble(&pack, &ctx).unwrap();
        (pack, a)
    }

    #[test]
    fn default_pack_assembles_every_kind_and_variant() {
        let pack = Pack::builtin();
        for kind in crate::packs::KINDS {
            for refs in [0, 2] {
                for outline in ["code", "model", "none"] {
                    for background in ["transparent", "flat"] {
                        let ctx = PromptContext {
                            refs,
                            outline: outline.into(),
                            background: background.into(),
                            ..PromptContext::for_kind(&pack, kind)
                        };
                        let a = assemble(&pack, &ctx).unwrap();
                        assert!(!a.text.contains('{'), "{kind} {outline}: placeholder left in {}", a.text);
                        for b in ["HEADER", "GOAL", "ROLES", "STYLE", "SUBJECT", "CAMERA", "AVOID", "OUTPUT"] {
                            assert!(a.sentences.iter().any(|s| s.block == b), "{kind}: block {b}");
                        }
                        assert!(is_english(&a.text));
                    }
                }
            }
        }
    }

    #[test]
    fn assembly_is_deterministic_and_matches_the_golden_file() {
        let (pack, a) = building();
        let again = assemble(&pack, &PromptContext::for_kind(&pack, "building")).unwrap();
        assert_eq!(a.text.as_bytes(), again.text.as_bytes());
        let golden = include_str!("../../../tests/fixtures/prompt/building-norefs.txt").replace("\r\n", "\n");
        assert_eq!(a.text, golden.trim_end_matches('\n'), "update tests/fixtures/prompt when the default pack changes");
        assert_eq!(a.template_version, "3");
        // Stylized keeps the template 2 text byte for byte
        let ctx = PromptContext { fidelity: "stylized".into(), ..PromptContext::for_kind(&pack, "building") };
        let styl = assemble(&pack, &ctx).unwrap();
        let golden = include_str!("../../../tests/fixtures/prompt/building-norefs-stylized.txt").replace("\r\n", "\n");
        assert_eq!(styl.text, golden.trim_end_matches('\n'));
    }

    #[test]
    fn strict_keeps_the_capture_and_stylized_may_decorate() {
        let pack = Pack::builtin();
        for kind in crate::packs::KINDS {
            for refs in [0, 2] {
                let strict = assemble(&pack, &PromptContext { refs, ..PromptContext::for_kind(&pack, kind) }).unwrap();
                let t = &strict.text;
                assert!(t.contains("This is a repaint, not a redesign"), "{kind}");
                assert!(t.contains("Add nothing that is not in the capture"), "{kind}");
                assert!(strict.sentences.iter().any(|s| s.id == "AVOID.additions" && s.locked));
                assert!(strict.sentences.iter().any(|s| s.id == "SUBJECT.keep" && s.locked && s.text.contains("how big each one is")));
                for word in ["chibi", "sparkles or hearts touching", "exaggerate", "enlarge windows", "thick base slab"] {
                    assert!(!t.contains(word), "{kind}: {word}");
                }
                let styl = assemble(&pack, &PromptContext { refs, fidelity: "stylized".into(), ..PromptContext::for_kind(&pack, kind) }).unwrap();
                assert!(styl.text.contains("sparkles or hearts"), "{kind}");
                assert!(!styl.text.contains("Add nothing that is not in the capture"));
            }
        }
        let prop = assemble(&pack, &PromptContext::for_kind(&pack, "prop")).unwrap();
        assert!(prop.text.contains("Keep any ground, base or stand that is part of the capture"));
    }

    #[test]
    fn the_camera_sentence_says_what_the_capture_was_taken_with() {
        let pack = Pack::builtin();
        let cam = |ctx: &PromptContext| assemble(&pack, ctx).unwrap().sentences.into_iter().find(|s| s.id == "CAMERA.view").unwrap().text;
        // The kind preset, unchanged: the same text as before the projection placeholder
        let base = cam(&PromptContext::for_kind(&pack, "building"));
        assert!(base.contains("about 30 degrees") && base.contains("about 28 degrees") && base.contains("the right side"), "{base}");
        assert!(base.contains("orthographic-like projection with parallel vertical edges, no wide-angle distortion"));
        // A capture turned the other way, looking down less, with a perspective lens
        let values = serde_json::json!({ "yaw": -40.0, "pitch": 12, "projection": "perspective", "fov": 25 });
        let ctx = PromptContext::for_kind(&pack, "building").with_capture(values.as_object().unwrap());
        let text = cam(&ctx);
        assert!(text.contains("about 12 degrees"), "{text}");
        assert!(text.contains("about 40 degrees"), "{text}");
        assert!(text.contains("the left side"), "{text}");
        assert!(text.contains("about a 25 degree field of view") && !text.contains("orthographic"), "{text}");
        // Nothing left unfilled
        assert!(!assemble(&pack, &ctx).unwrap().text.contains('{'));
    }

    #[test]
    fn note_additions_become_an_exception_after_the_locked_rule() {
        let (pack, fixed) = building();
        let edits = Edits {
            add: vec![AddEdit { block: "SUBJECT".into(), text: "A thick layer of white snow covers the roof.".into() }],
            additions: vec!["snow on the roof".into(), "a logo".into()],
            outline: "keep".into(),
            camera: "keep".into(),
            ..Edits::default()
        };
        let r = apply_edits(&pack, &fixed, &edits).unwrap();
        assert_eq!(r.additions, ["snow on the roof"]);
        assert!(r.rejected.iter().any(|x| x.reason == "forbidden:logo"));
        let i = r.sentences.iter().position(|s| s.id == "AVOID.additions").unwrap();
        assert_eq!(r.sentences[i + 1].id, "AVOID.allow");
        assert!(r.text.contains("Add nothing that is not in the capture: no sparkles, stars, hearts, flowers, leaves, bubbles, extra props, emblems, signs or effects. Except what the note asks for: snow on the roof."));
        // Stylized has no such rule: no exception sentence
        let styl = assemble(&pack, &PromptContext { fidelity: "stylized".into(), ..PromptContext::for_kind(&pack, "building") }).unwrap();
        let r = apply_edits(&pack, &styl, &edits).unwrap();
        assert!(!r.sentences.iter().any(|s| s.id == "AVOID.allow"));
        // A correction for a retry goes at the end of SUBJECT, unlocked
        let (s, text) = with_sentence(&pack, &fixed.sentences, "SUBJECT", "correction", "Corrections from the previous try: do not add sparkles.");
        assert!(check_locked(&fixed, &s).is_ok());
        assert!(text.contains("No characters or people. Corrections from the previous try: do not add sparkles."));
    }

    #[test]
    fn camera_sentence_carries_the_capture_values() {
        let pack = Pack::builtin();
        let ctx = PromptContext { yaw: -28.0, pitch: 30.0, ..PromptContext::for_kind(&pack, "building") };
        let a = assemble(&pack, &ctx).unwrap();
        let cam = a.sentences.iter().find(|s| s.id == "CAMERA.view").unwrap();
        assert!(cam.locked);
        assert!(cam.text.contains("about 30 degrees"), "{}", cam.text);
        assert!(cam.text.contains("about 28 degrees") && cam.text.contains("left side"), "{}", cam.text);
        let prop = assemble(&pack, &PromptContext::for_kind(&pack, "prop")).unwrap();
        let cam = prop.sentences.iter().find(|s| s.id == "CAMERA.view").unwrap();
        assert!(cam.text.contains("about 18 degrees") && cam.text.contains("about 25 degrees"), "{}", cam.text);
    }

    #[test]
    fn variants_switch_roles_outline_and_output() {
        let pack = Pack::builtin();
        let base = PromptContext::for_kind(&pack, "prop");
        let refs = assemble(&pack, &PromptContext { refs: 2, ..base.clone() }).unwrap();
        assert!(refs.text.contains("Input images 2 to 3 are style references"));
        let one = assemble(&pack, &PromptContext { refs: 1, ..base.clone() }).unwrap();
        assert!(one.text.contains("Input image 2 is a style reference:"), "{}", one.text);
        let model = assemble(&pack, &PromptContext { outline: "model".into(), ..base.clone() }).unwrap();
        assert!(model.text.contains("(#753719)") && model.text.contains("1.45%"));
        let code = assemble(&pack, &base).unwrap();
        assert!(code.text.contains("No outline:"));
        let flat = assemble(&pack, &PromptContext { background: "flat".into(), key_color: "#00FF00".into(), ..base }).unwrap();
        assert!(flat.text.contains("solid #00FF00 background"));
        assert!(!flat.text.contains("transparent background"));
    }

    #[test]
    fn empty_notes() {
        assert!(note_is_empty(""));
        assert!(note_is_empty("  \r\n\t \n"));
        assert!(!note_is_empty(" 겨울 "));
    }

    fn edits(v: Value) -> Edits {
        parse_edits(&v).unwrap()
    }

    #[test]
    fn edits_replace_unlocked_and_add() {
        let (pack, fixed) = building();
        let e = edits(json!({
            "replace": [{ "id": "STYLE.color", "text": "Color: blue roof, cool winter palette with soft snow white accents; no haze, no fog, no bloom." }],
            "add": [{ "block": "SUBJECT", "text": "A light layer of snow sits on the roof." }],
            "avoidAdd": ["summer flowers"],
            "outline": "keep", "camera": "keep", "rejected": [], "summaryKo": "지붕을 파랗게, 겨울 느낌을 더했습니다."
        }));
        let r = apply_edits(&pack, &fixed, &e).unwrap();
        assert!(r.changed);
        let color = r.sentences.iter().find(|s| s.id == "STYLE.color").unwrap();
        assert_eq!(color.status, Status::Replaced);
        assert!(color.before.as_deref().unwrap().starts_with("Color: bright"));
        let add = r.sentences.iter().find(|s| s.id == "SUBJECT.add1").unwrap();
        assert_eq!(add.status, Status::Added);
        // added at the end of its block, before CAMERA
        let at = r.sentences.iter().position(|s| s.id == "SUBJECT.add1").unwrap();
        assert_eq!(r.sentences[at + 1].id, "CAMERA.view");
        assert!(r.text.contains("Also avoid: summer flowers."));
        check_locked(&fixed, &r.sentences).unwrap();
        for s in fixed.sentences.iter().filter(|s| s.locked) {
            assert!(r.text.contains(&s.text), "locked sentence kept: {}", s.id);
        }
    }

    #[test]
    fn locked_and_unknown_ids_are_rejected_not_applied() {
        let (pack, fixed) = building();
        let e = edits(json!({
            "replace": [
                { "id": "OUTPUT.spec", "text": "Output: a square icon on a white floor." },
                { "id": "CAMERA.view", "text": "Camera from straight above." },
                { "id": "STYLE.nope", "text": "Something else." }
            ],
            "add": [{ "block": "OUTPUT", "text": "One more output rule." }],
            "avoidAdd": [], "outline": "keep", "camera": "keep", "rejected": [], "summaryKo": ""
        }));
        let r = apply_edits(&pack, &fixed, &e).unwrap();
        assert!(!r.changed);
        assert_eq!(r.text, fixed.text);
        let reasons: Vec<&str> = r.rejected.iter().map(|x| x.reason.as_str()).collect();
        assert_eq!(reasons, ["locked:OUTPUT.spec", "locked:CAMERA.view", "unknownId:STYLE.nope", "notAddable:OUTPUT"]);
        assert!(r.rejected.iter().all(|x| x.by == "code"));
    }

    #[test]
    fn locked_check_catches_any_change() {
        let (_, fixed) = building();
        let mut changed = fixed.sentences.clone();
        let i = changed.iter().position(|s| s.id == "AVOID.tech").unwrap();
        changed[i].text.push(' ');
        assert_eq!(check_locked(&fixed, &changed).unwrap_err(), "lockedChanged:AVOID.tech");
        let mut dropped = fixed.sentences.clone();
        dropped.retain(|s| s.id != "HEADER.verbatim");
        assert!(check_locked(&fixed, &dropped).is_err());
    }

    #[test]
    fn forbidden_words_and_banned_terms_are_taken_out() {
        let (mut pack, fixed) = building();
        pack.doc["bannedTerms"] = json!(["Sparkle Town", "Mr Bolt"]);
        let e = edits(json!({
            "replace": [{ "id": "STYLE.look", "text": "Style: in the look of sparkle town games." }],
            "add": [
                { "block": "STYLE", "text": "Put a snowy village background behind it." },
                { "block": "SUBJECT", "text": "Add the game logo on the roof." },
                { "block": "SUBJECT", "text": "Add a red ribbon on the door." }
            ],
            "avoidAdd": ["mr bolt"], "outline": "keep", "camera": "keep",
            "rejected": [{ "part": "게임 로고", "reason": "글자·로고 금지" }], "summaryKo": ""
        }));
        let r = apply_edits(&pack, &fixed, &e).unwrap();
        let reasons: Vec<&str> = r.rejected.iter().map(|x| x.reason.as_str()).collect();
        assert_eq!(reasons, ["글자·로고 금지", "banned:Sparkle Town", "forbidden:background", "forbidden:logo", "banned:Mr Bolt"]);
        assert_eq!(r.rejected[0].by, "model");
        assert!(r.text.contains("Add a red ribbon on the door."));
        assert!(!r.text.to_lowercase().contains("logo on the roof"));
        // "texture" is not "text"
        assert!(forbidden_word("no photographic texture").is_none());
    }

    #[test]
    fn parse_checks_counts_lengths_and_language() {
        let ok = json!({ "replace": [], "add": [], "avoidAdd": [], "outline": "keep", "camera": "keep", "rejected": [], "summaryKo": "변경 없음" });
        assert!(parse_edits(&ok).is_ok());
        let mut many = ok.clone();
        many["add"] = json!((0..5).map(|i| json!({ "block": "STYLE", "text": format!("Extra {i}.") })).collect::<Vec<_>>());
        assert!(parse_edits(&many).unwrap_err().contains("too many"));
        let mut long = ok.clone();
        long["avoidAdd"] = json!(["x".repeat(241)]);
        assert!(parse_edits(&long).unwrap_err().contains("longer"));
        let mut korean = ok.clone();
        korean["replace"] = json!([{ "id": "STYLE.color", "text": "지붕을 파란색으로 바꿉니다" }]);
        assert!(parse_edits(&korean).unwrap_err().contains("English"));
        assert!(parse_edits(&json!("text")).is_err());
        assert!(parse_edits(&json!({ "replace": "nope" })).is_err());
        assert!(is_english("Color: café crème roof"));
    }

    #[test]
    fn outline_off_and_camera_are_reported_not_applied() {
        let (pack, fixed) = building();
        let e = edits(json!({ "replace": [], "add": [], "avoidAdd": [], "outline": "off", "camera": "front", "rejected": [], "summaryKo": "" }));
        let r = apply_edits(&pack, &fixed, &e).unwrap();
        assert_eq!((r.outline.as_str(), r.camera.as_str()), ("off", "front"));
        assert_eq!(r.text, fixed.text);
    }

    #[test]
    fn rewrite_prompt_lists_sentences_and_the_note() {
        let (_, fixed) = building();
        let p = rewrite_prompt(&fixed, "  지붕을 파란색으로  ", None);
        assert!(p.contains("\"note\": \"지붕을 파란색으로\""));
        assert!(p.contains("\"id\": \"STYLE.color\""));
        assert!(p.contains("\"locked\": true"));
        assert!(rewrite_prompt(&fixed, "x", Some("too many items")).contains("could not be used (too many items)"));
    }

    #[test]
    fn generator_text_wraps_the_prompt_bytes() {
        let (pack, fixed) = building();
        let g = generator_text(&pack, "transparent", &fixed.text);
        assert!(g.starts_with("Use your built-in image generation tool exactly once."));
        assert!(g.contains("Request a transparent background."));
        let start = g.find("PROMPT: <<<\n").unwrap() + "PROMPT: <<<\n".len();
        let end = g.rfind("\n>>>\n").unwrap();
        assert_eq!(&g.as_bytes()[start..end], fixed.text.as_bytes());
    }

    #[test]
    fn schemas_parse() {
        let r: Value = serde_json::from_str(REWRITE_SCHEMA).unwrap();
        assert_eq!(r["additionalProperties"], json!(false));
        let props: Vec<&String> = r["properties"].as_object().unwrap().keys().collect();
        let req: Vec<&str> = r["required"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(props.len(), req.len(), "strict schemas need every key required");
        let _: Value = serde_json::from_str(REVIEW_SCHEMA).unwrap();
    }
}
