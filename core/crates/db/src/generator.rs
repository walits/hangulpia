use crate::kana_hangul::hiragana_to_hangul;
use crate::vocab::{build_vocab, Category, Pos, VocabEntry};
use std::collections::{HashMap, HashSet};

/// Dynamically generated word with surface, reading, and hangul
#[derive(Debug, Clone)]
pub struct GenWord {
    pub surface: String,
    pub reading: String,
    pub hangul: String,
}

/// Dynamically generated sentence with category and word sequence
#[derive(Debug, Clone)]
pub struct GenSentence {
    pub category: String,
    pub words: Vec<GenWord>,
}

/// Simple linear congruential generator for deterministic, fast random generation
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn next_usize(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.next_u64() % max as u64) as usize
    }
}

/// Slot type for template patterns
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotType {
    Noun,
    Verb,
    VerbMasu,
    VerbTe,
    VerbTa,
    VerbNai,
    IAdjective,
    NaAdjective,
    Adverb,
    Time,
    Place,
    Food,
    Action,
    Subject,
}

/// Constraints for filling a slot
#[derive(Debug, Clone, Copy)]
struct SlotConstraint {
    slot_type: SlotType,
    preferred_category: Option<Category>,
}

/// A sentence template with slots to fill
#[derive(Debug, Clone)]
struct Template {
    name: &'static str,
    pattern: Vec<TemplateItem>,
    category: &'static str,
}

/// Items in a template pattern
#[derive(Debug, Clone)]
enum TemplateItem {
    Literal(String),
    Slot(SlotConstraint),
}

/// Build sentence templates with structural patterns
fn build_templates() -> Vec<Template> {
    vec![
        // Basic identification patterns
        Template {
            name: "X is Y",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Subject,
                    preferred_category: None,
                }),
                TemplateItem::Literal("は".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("です".to_string()),
            ],
            category: "copula",
        },
        // I-adjective patterns
        Template {
            name: "X is adj",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Subject,
                    preferred_category: None,
                }),
                TemplateItem::Literal("が".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::IAdjective,
                    preferred_category: None,
                }),
                TemplateItem::Literal("です".to_string()),
            ],
            category: "i-adjective",
        },
        // Na-adjective patterns
        Template {
            name: "X is na-adj",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Subject,
                    preferred_category: None,
                }),
                TemplateItem::Literal("は".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::NaAdjective,
                    preferred_category: None,
                }),
                TemplateItem::Literal("です".to_string()),
            ],
            category: "na-adjective",
        },
        // Verb patterns - basic
        Template {
            name: "Do X",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("を".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbMasu,
                    preferred_category: None,
                }),
            ],
            category: "verb-transitive",
        },
        // Verb patterns - location
        Template {
            name: "Do X at location",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Place,
                    preferred_category: Some(Category::Place),
                }),
                TemplateItem::Literal("で".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("を".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbMasu,
                    preferred_category: None,
                }),
            ],
            category: "verb-locative",
        },
        // Verb patterns - to/at target
        Template {
            name: "Do at target",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("に".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbMasu,
                    preferred_category: None,
                }),
            ],
            category: "verb-dative",
        },
        // Conjunction pattern - X and Y
        Template {
            name: "Do X and Y",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("と".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("を".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbMasu,
                    preferred_category: None,
                }),
            ],
            category: "conjunction",
        },
        // Possessive pattern
        Template {
            name: "X's Y is adj",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Subject,
                    preferred_category: None,
                }),
                TemplateItem::Literal("の".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("が".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::IAdjective,
                    preferred_category: None,
                }),
                TemplateItem::Literal("です".to_string()),
            ],
            category: "possessive",
        },
        // Adverbial modification
        Template {
            name: "Very adj noun",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Adverb,
                    preferred_category: None,
                }),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::IAdjective,
                    preferred_category: None,
                }),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("です".to_string()),
            ],
            category: "adverbial",
        },
        // Time patterns
        Template {
            name: "At time, do action",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Time,
                    preferred_category: Some(Category::Time),
                }),
                TemplateItem::Literal("に".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("を".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbMasu,
                    preferred_category: None,
                }),
            ],
            category: "temporal",
        },
        // Range pattern
        Template {
            name: "From X to Y, do verb",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Place,
                    preferred_category: Some(Category::Place),
                }),
                TemplateItem::Literal("から".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Place,
                    preferred_category: Some(Category::Place),
                }),
                TemplateItem::Literal("まで".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbMasu,
                    preferred_category: None,
                }),
            ],
            category: "range",
        },
        // Also pattern
        Template {
            name: "X also does",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Subject,
                    preferred_category: None,
                }),
                TemplateItem::Literal("も".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbMasu,
                    preferred_category: None,
                }),
            ],
            category: "inclusive",
        },
        // Te-form conjunction
        Template {
            name: "Do X and do Y",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("を".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbTe,
                    preferred_category: None,
                }),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("を".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbMasu,
                    preferred_category: None,
                }),
            ],
            category: "te-conjunction",
        },
        // Negative pattern
        Template {
            name: "X doesn't do",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Subject,
                    preferred_category: None,
                }),
                TemplateItem::Literal("は".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbNai,
                    preferred_category: None,
                }),
            ],
            category: "negative",
        },
        // Past tense pattern
        Template {
            name: "X did",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Subject,
                    preferred_category: None,
                }),
                TemplateItem::Literal("は".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbTa,
                    preferred_category: None,
                }),
            ],
            category: "past",
        },
        // Double object pattern
        Template {
            name: "Give X to Y",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("に".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("を".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::VerbMasu,
                    preferred_category: None,
                }),
            ],
            category: "ditransitive",
        },
        // Subject has property
        Template {
            name: "X has property Y",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Subject,
                    preferred_category: None,
                }),
                TemplateItem::Literal("は".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("が".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::NaAdjective,
                    preferred_category: None,
                }),
                TemplateItem::Literal("です".to_string()),
            ],
            category: "property",
        },
        // Comparison pattern
        Template {
            name: "X is more adj than Y",
            pattern: vec![
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Subject,
                    preferred_category: None,
                }),
                TemplateItem::Literal("は".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::Noun,
                    preferred_category: None,
                }),
                TemplateItem::Literal("より".to_string()),
                TemplateItem::Slot(SlotConstraint {
                    slot_type: SlotType::IAdjective,
                    preferred_category: None,
                }),
                TemplateItem::Literal("です".to_string()),
            ],
            category: "comparative",
        },
    ]
}

/// Convert verb reading to masu form
fn to_masu_form(reading: &str) -> String {
    let hiragana = reading.trim();

    // Irregular verbs first (before checking endings)
    match hiragana {
        "する" => return "します".to_string(),
        "くる" => return "きます".to_string(),
        "いく" => return "いきます".to_string(),
        _ => {}
    }

    let chars: Vec<char> = hiragana.chars().collect();

    if chars.is_empty() {
        return format!("{}ます", hiragana);
    }

    let last_char = chars[chars.len() - 1];
    let stem: String = chars[..chars.len() - 1].iter().collect();

    // Godan verbs (consonant stem)
    match last_char {
        'く' => format!("{}きます", stem),
        'ぐ' => format!("{}ぎます", stem),
        'す' => format!("{}します", stem),
        'つ' => format!("{}ちます", stem),
        'ぬ' => format!("{}にます", stem),
        'ぶ' => format!("{}びます", stem),
        'む' => format!("{}みます", stem),
        'ふ' => format!("{}ひます", stem),
        'う' => format!("{}います", stem),
        'る' => format!("{}ます", stem),
        _ => format!("{}ます", hiragana),
    }
}

/// Convert verb reading to te form
fn to_te_form(reading: &str) -> String {
    let hiragana = reading.trim();

    // Irregular verbs first
    match hiragana {
        "する" => return "して".to_string(),
        "くる" => return "きて".to_string(),
        "いく" => return "いって".to_string(),
        _ => {}
    }

    let chars: Vec<char> = hiragana.chars().collect();

    if chars.is_empty() {
        return format!("{}て", hiragana);
    }

    let last_char = chars[chars.len() - 1];
    let stem: String = chars[..chars.len() - 1].iter().collect();

    match last_char {
        'く' => format!("{}いて", stem),
        'ぐ' => format!("{}いで", stem),
        'す' => format!("{}して", stem),
        'つ' => format!("{}って", stem),
        'ぬ' => format!("{}んで", stem),
        'ぶ' => format!("{}んで", stem),
        'む' => format!("{}んで", stem),
        'ふ' => format!("{}って", stem),
        'う' => format!("{}って", stem),
        'る' => format!("{}て", stem),
        _ => format!("{}て", hiragana),
    }
}

/// Convert verb reading to ta form
fn to_ta_form(reading: &str) -> String {
    let hiragana = reading.trim();

    // Irregular verbs first
    match hiragana {
        "する" => return "した".to_string(),
        "くる" => return "きた".to_string(),
        "いく" => return "いった".to_string(),
        _ => {}
    }

    let chars: Vec<char> = hiragana.chars().collect();

    if chars.is_empty() {
        return format!("{}た", hiragana);
    }

    let last_char = chars[chars.len() - 1];
    let stem: String = chars[..chars.len() - 1].iter().collect();

    match last_char {
        'く' => format!("{}いた", stem),
        'ぐ' => format!("{}いだ", stem),
        'す' => format!("{}した", stem),
        'つ' => format!("{}った", stem),
        'ぬ' => format!("{}んだ", stem),
        'ぶ' => format!("{}んだ", stem),
        'む' => format!("{}んだ", stem),
        'ふ' => format!("{}った", stem),
        'う' => format!("{}った", stem),
        'る' => format!("{}た", stem),
        _ => format!("{}た", hiragana),
    }
}

/// Convert verb reading to nai form
fn to_nai_form(reading: &str) -> String {
    let hiragana = reading.trim();

    // Irregular verbs first
    match hiragana {
        "する" => return "しない".to_string(),
        "くる" => return "こない".to_string(),
        "いく" => return "いかない".to_string(),
        _ => {}
    }

    let chars: Vec<char> = hiragana.chars().collect();

    if chars.is_empty() {
        return format!("{}ない", hiragana);
    }

    let last_char = chars[chars.len() - 1];
    let stem: String = chars[..chars.len() - 1].iter().collect();

    // Godan verbs and ichidan verbs
    match last_char {
        'く' => format!("{}かない", stem),
        'ぐ' => format!("{}がない", stem),
        'す' => format!("{}さない", stem),
        'つ' => format!("{}たない", stem),
        'ぬ' => format!("{}なない", stem),
        'ぶ' => format!("{}ばない", stem),
        'む' => format!("{}まない", stem),
        'ふ' => format!("{}わない", stem),
        'う' => format!("{}わない", stem),
        'る' => format!("{}ない", stem),
        _ => format!("{}ない", hiragana),
    }
}

/// Select a random word from vocab based on slot type and constraints
fn select_word_for_slot<'a>(
    slot: &SlotConstraint,
    vocab: &'a [VocabEntry],
    category_index: &HashMap<Category, Vec<&'a VocabEntry>>,
    rng: &mut Rng,
) -> Option<&'a VocabEntry> {
    let candidates: Vec<_> = match slot.slot_type {
        SlotType::Noun => vocab
            .iter()
            .filter(|e| matches!(e.pos, Pos::Noun))
            .collect(),
        SlotType::Verb => vocab
            .iter()
            .filter(|e| matches!(e.pos, Pos::Verb))
            .collect(),
        SlotType::VerbMasu => vocab
            .iter()
            .filter(|e| matches!(e.pos, Pos::Verb))
            .collect(),
        SlotType::VerbTe => vocab
            .iter()
            .filter(|e| matches!(e.pos, Pos::Verb))
            .collect(),
        SlotType::VerbTa => vocab
            .iter()
            .filter(|e| matches!(e.pos, Pos::Verb))
            .collect(),
        SlotType::VerbNai => vocab
            .iter()
            .filter(|e| matches!(e.pos, Pos::Verb))
            .collect(),
        SlotType::IAdjective => vocab
            .iter()
            .filter(|e| matches!(e.pos, Pos::IAdjective))
            .collect(),
        SlotType::NaAdjective => vocab
            .iter()
            .filter(|e| matches!(e.pos, Pos::IAdjective))
            .collect(),
        SlotType::Adverb => vocab
            .iter()
            .filter(|e| matches!(e.pos, Pos::Adverb))
            .collect(),
        SlotType::Time => {
            if let Some(preferred) = slot.preferred_category {
                category_index
                    .get(&preferred)
                    .map(|v| v.clone())
                    .unwrap_or_default()
            } else {
                vocab
                    .iter()
                    .filter(|e| matches!(e.category, Category::Time))
                    .collect()
            }
        }
        SlotType::Place => {
            if let Some(preferred) = slot.preferred_category {
                category_index
                    .get(&preferred)
                    .map(|v| v.clone())
                    .unwrap_or_default()
            } else {
                vocab
                    .iter()
                    .filter(|e| matches!(e.category, Category::Place))
                    .collect()
            }
        }
        SlotType::Food => category_index
            .get(&Category::Food)
            .map(|v| v.clone())
            .unwrap_or_default(),
        SlotType::Action => vocab
            .iter()
            .filter(|e| matches!(e.pos, Pos::Verb))
            .collect(),
        SlotType::Subject => vocab
            .iter()
            .filter(|e| matches!(e.pos, Pos::Noun))
            .collect(),
    };

    if candidates.is_empty() {
        return None;
    }

    let idx = rng.next_usize(candidates.len());
    Some(candidates[idx])
}

/// Fill a single slot in a template with an appropriate word
fn fill_slot(
    slot: &SlotConstraint,
    vocab: &[VocabEntry],
    category_index: &HashMap<Category, Vec<&VocabEntry>>,
    rng: &mut Rng,
) -> Option<GenWord> {
    let entry = select_word_for_slot(slot, vocab, category_index, rng)?;

    let reading = entry.reading.to_string();
    let hangul = hiragana_to_hangul(entry.reading);

    let surface = match slot.slot_type {
        SlotType::VerbMasu => to_masu_form(entry.reading),
        SlotType::VerbTe => to_te_form(entry.reading),
        SlotType::VerbTa => to_ta_form(entry.reading),
        SlotType::VerbNai => to_nai_form(entry.reading),
        _ => entry.surface.to_string(),
    };

    Some(GenWord {
        surface,
        reading,
        hangul,
    })
}

/// Expand a template pattern into a sentence
fn expand_template(
    template: &Template,
    vocab: &[VocabEntry],
    category_index: &HashMap<Category, Vec<&VocabEntry>>,
    rng: &mut Rng,
) -> Option<GenSentence> {
    let mut words: Vec<GenWord> = Vec::new();

    for item in &template.pattern {
        match item {
            TemplateItem::Literal(lit) => {
                if !words.is_empty() && !lit.is_empty() {
                    // Append to previous word's surface form instead of creating new word
                    words.last_mut().unwrap().surface.push_str(lit);
                } else {
                    // Create new word for punctuation/particles
                    words.push(GenWord {
                        surface: lit.clone(),
                        reading: lit.clone(),
                        hangul: hiragana_to_hangul(lit),
                    });
                }
            }
            TemplateItem::Slot(constraint) => {
                if let Some(word) = fill_slot(constraint, vocab, category_index, rng) {
                    words.push(word);
                } else {
                    return None;
                }
            }
        }
    }

    Some(GenSentence {
        category: template.category.to_string(),
        words,
    })
}

/// Build a category index for faster lookups
fn build_category_index(vocab: &[VocabEntry]) -> HashMap<Category, Vec<&VocabEntry>> {
    let mut index = HashMap::new();
    for entry in vocab {
        index
            .entry(entry.category)
            .or_insert_with(Vec::new)
            .push(entry);
    }
    index
}

/// Generate a corpus of Japanese sentences
pub fn generate_corpus(count: usize) -> Vec<GenSentence> {
    let vocab = build_vocab();
    generate_corpus_inner(&vocab, count)
}

/// Generate a corpus using a custom vocabulary set
pub fn generate_corpus_with_vocab(vocab: &[VocabEntry], count: usize) -> Vec<GenSentence> {
    generate_corpus_inner(vocab, count)
}

fn generate_corpus_inner(vocab: &[VocabEntry], count: usize) -> Vec<GenSentence> {
    generate_corpus_seeded(vocab, count, 42)
}

/// Generate corpus with a specific seed (for test data isolation)
pub fn generate_corpus_with_seed(vocab: &[VocabEntry], count: usize, seed: u64) -> Vec<GenSentence> {
    generate_corpus_seeded(vocab, count, seed)
}

fn generate_corpus_seeded(vocab: &[VocabEntry], count: usize, seed: u64) -> Vec<GenSentence> {
    let templates = build_templates();
    let category_index = build_category_index(vocab);

    let mut rng = Rng::new(seed);
    let mut corpus = Vec::with_capacity(count);

    let mut attempts = 0;
    while corpus.len() < count && attempts < count * 3 {
        let template_idx = rng.next_usize(templates.len());
        let template = &templates[template_idx];

        if let Some(sentence) = expand_template(template, vocab, &category_index, &mut rng) {
            corpus.push(sentence);
        }

        attempts += 1;
    }

    corpus.truncate(count);
    corpus
}

/// Callback-based chunk generation: generates `total` sentences in chunks of `chunk_size`,
/// calling `on_chunk` for each chunk. This avoids holding all sentences in memory.
pub fn generate_corpus_chunked<F>(
    vocab: &[VocabEntry],
    total: usize,
    chunk_size: usize,
    mut on_chunk: F,
) where
    F: FnMut(&[GenSentence]),
{
    let templates = build_templates();
    let category_index = build_category_index(vocab);
    let mut rng = Rng::new(42);
    let mut generated = 0usize;
    let mut attempts = 0usize;

    let mut chunk = Vec::with_capacity(chunk_size);

    while generated < total && attempts < total * 3 {
        let template_idx = rng.next_usize(templates.len());
        let template = &templates[template_idx];

        if let Some(sentence) = expand_template(template, vocab, &category_index, &mut rng) {
            chunk.push(sentence);
            generated += 1;

            if chunk.len() >= chunk_size {
                on_chunk(&chunk);
                chunk.clear();
            }
        }
        attempts += 1;
    }

    // Flush remaining
    if !chunk.is_empty() {
        on_chunk(&chunk);
    }
}

/// Generate statistics about a corpus
pub fn corpus_stats(corpus: &[GenSentence]) -> String {
    let mut unique_words = HashSet::new();
    let mut category_counts = HashMap::new();
    let mut total_tokens = 0;

    for sentence in corpus {
        for word in &sentence.words {
            unique_words.insert(word.surface.clone());
            total_tokens += 1;
        }
        *category_counts.entry(sentence.category.clone()).or_insert(0) += 1;
    }

    let mut stats = format!(
        "Corpus Statistics:\n  Total sentences: {}\n  Total tokens: {}\n  Unique words: {}\n",
        corpus.len(),
        total_tokens,
        unique_words.len()
    );

    stats.push_str("  Category distribution:\n");
    let mut sorted_cats: Vec<_> = category_counts.iter().collect();
    sorted_cats.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (cat, count) in sorted_cats {
        stats.push_str(&format!("    {}: {}\n", cat, count));
    }

    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generation_count() {
        let corpus = generate_corpus(1000);
        assert_eq!(corpus.len(), 1000);
    }

    #[test]
    fn test_deterministic() {
        let mut rng1 = Rng::new(12345);
        let mut rng2 = Rng::new(12345);

        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_unique_words() {
        let corpus = generate_corpus(1000);
        let mut unique = HashSet::new();
        for sentence in &corpus {
            for word in &sentence.words {
                unique.insert(word.surface.clone());
            }
        }
        assert!(unique.len() > 200, "Expected >200 unique words, got {}", unique.len());
    }

    #[test]
    fn test_hangul_generated() {
        let corpus = generate_corpus(100);
        for sentence in &corpus {
            for word in &sentence.words {
                assert!(
                    !word.hangul.is_empty(),
                    "Hangul empty for surface: {}",
                    word.surface
                );
            }
        }
    }

    #[test]
    fn test_masu_form() {
        assert_eq!(to_masu_form("いく"), "いきます");
        assert_eq!(to_masu_form("たべる"), "たべます");
        assert_eq!(to_masu_form("する"), "します");
        assert_eq!(to_masu_form("くる"), "きます");
    }

    #[test]
    fn test_te_form() {
        assert_eq!(to_te_form("いく"), "いって");
        assert_eq!(to_te_form("たべる"), "たべて");
        assert_eq!(to_te_form("する"), "して");
    }

    #[test]
    fn test_ta_form() {
        assert_eq!(to_ta_form("いく"), "いった");
        assert_eq!(to_ta_form("たべる"), "たべた");
        assert_eq!(to_ta_form("する"), "した");
    }

    #[test]
    fn test_nai_form() {
        assert_eq!(to_nai_form("いく"), "いかない");
        assert_eq!(to_nai_form("たべる"), "たべない");
        assert_eq!(to_nai_form("する"), "しない");
    }

    #[test]
    fn test_template_expansion() {
        let templates = build_templates();
        assert!(templates.len() > 15, "Expected >15 templates");
    }

    #[test]
    fn test_sentences_have_category() {
        let corpus = generate_corpus(100);
        for sentence in corpus {
            assert!(!sentence.category.is_empty());
            assert!(!sentence.words.is_empty());
        }
    }
}
