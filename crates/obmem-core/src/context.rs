//! `memory_context(task)`: the notes most relevant to a task description, within a size budget.
//! `Store::search` does the searching; this only turns free text into an OR query (Obsidian ANDs
//! words, and many scripts have no spaces) and ranks the hits, which Obsidian search does not:
//! by the terms a note has. A rare term counts for more than one most notes have ([`weight`]), and
//! a note that is long needs a term more often ([`share`]).
//!
//! The caller is an agent that can search again, so the answer also says where to go next: which
//! terms each note matched, which terms no note contains, and the notes one link away from the
//! matches ([`Recall`]).
//!
//! Terms are cut by *script*, not by language, so any language gets usable terms without a
//! per-language word list:
//! - scripts written with spaces (Latin incl. accents, Cyrillic, Greek, Arabic, Hebrew, Indic,
//!   …): words of 3+ characters, combining marks kept inside the word;
//! - Han: overlapping character bigrams (a two-character run stays whole);
//! - Katakana: the whole run (mostly loanwords: トークン); Hiragana: dropped (particles, endings);
//! - Hangul: bigrams, because particles attach to nouns (방식을 → 방식, 식을);
//! - Thai, Lao, Khmer, Myanmar (no spaces): bigrams of character clusters (base + marks).
//!
//! Matching stays literal: "rotation" does not find 輪替, and an inflected form finds only what
//! starts with it. Obsidian's search finds a word anywhere, the ranking only where a word starts
//! ([`count`]).

use crate::catalog::Hit;
use crate::note::{Note, Scope};
use crate::store::Store;
use anyhow::Result;

const MAX_TERMS: usize = 16;
/// How many notes the search hands to the ranking. Obsidian's search does not rank, so a note
/// it leaves out is lost before it is scored.
const CANDIDATES: usize = 500;
/// How many of the best matches have their links followed. A weak match's neighbours are
/// rarely about the task.
const FOLLOWED: usize = 3;
/// How many notes one link away are listed.
const MAX_LINKED: usize = 5;
/// The vault's map of content. It links to every note, so it says nothing about which notes
/// belong together, and it has most terms of any task.
const INDEX: &str = "Index.md";
const EN_STOP: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "into", "how", "what", "why", "when",
    "where", "can", "should", "would", "could", "need", "want", "make", "add", "fix", "use",
    "using", "please", "about", "some",
];
/// Very common bigrams in Chinese task descriptions that say nothing about the subject.
const ZH_STOP: &[&str] = &[
    "我們", "這個", "那個", "一個", "可以", "怎麼", "什麼", "如何", "需要", "幫我", "的話", "一下",
    "是不", "不是", "有沒", "沒有", "現在", "然後", "因為", "所以", "還是", "或是", "能不", "不能",
    "直接", "看看", "要怎",
];

/// Pronouns and particles: a bigram containing one is almost never a topic word.
const ZH_FUNCTION_CHARS: &str = "我你他她它們的了嗎吧呢啊喔哦呀嘛";

fn keep_bigram(t: &str) -> bool {
    !ZH_STOP.contains(&t) && !t.chars().any(|c| ZH_FUNCTION_CHARS.contains(c))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Letters and digits of scripts written with spaces.
    Word,
    Han,
    Hiragana,
    Katakana,
    Hangul,
    /// Southeast Asian scripts written without spaces.
    Sea,
    Separator,
}

fn class(c: char) -> Class {
    let u = c as u32;
    match u {
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x3134F => Class::Han,
        0x3041..=0x309F => Class::Hiragana,
        0x30A0..=0x30FA | 0x30FC..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => Class::Katakana,
        0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F => Class::Hangul,
        0x0E00..=0x0EFF | 0x1000..=0x109F | 0x1780..=0x17FF => Class::Sea,
        // Indic blocks are letters, vowel signs and viramas, except the danda punctuation
        0x0964 | 0x0965 => Class::Separator,
        0x0900..=0x0DFF => Class::Word,
        _ if c.is_alphanumeric() || c == '_' || c == '-' || is_combining(c) => Class::Word,
        _ => Class::Separator,
    }
}

/// Combining marks that `char::is_alphanumeric` does not cover (diacritics in decomposed text,
/// Hebrew points, Arabic harakat, joiners): they belong to the word they sit on.
fn is_combining(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05C7 | 0x0610..=0x061A | 0x064B..=0x065F
        | 0x0670 | 0x06D6..=0x06ED | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200C | 0x200D
        | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Thai / Lao / Khmer / Myanmar vowel and tone marks: part of the preceding character cluster.
fn is_sea_mark(c: char) -> bool {
    matches!(c as u32,
        0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E | 0x0EB1 | 0x0EB4..=0x0EBC | 0x0EC8..=0x0ECE
        | 0x102B..=0x103E | 0x1056..=0x1059 | 0x105E..=0x1060 | 0x1062..=0x1064 | 0x1067..=0x106D
        | 0x1071..=0x1074 | 0x1082..=0x108D | 0x108F | 0x109A..=0x109D | 0x17B4..=0x17D3 | 0x17DD)
}

fn bigrams(units: &[String]) -> Vec<String> {
    match units.len() {
        0 | 1 => vec![],
        2 => vec![units.concat()],
        _ => units.windows(2).map(|w| w.concat()).collect(),
    }
}

/// Free text → search terms (see the module docs for the rules per script).
pub fn task_terms(task: &str) -> Vec<String> {
    // 1. runs of one class; Sea runs are split into clusters, everything else into characters
    let mut runs: Vec<(Class, Vec<String>)> = Vec::new();
    for c in task.chars() {
        let mut k = class(c);
        if is_sea_mark(c) {
            k = Class::Sea;
        }
        match runs.last_mut() {
            Some((Class::Sea, units)) if k == Class::Sea && is_sea_mark(c) && !units.is_empty() => {
                units.last_mut().unwrap().push(c)
            }
            Some((last, units)) if *last == k && k != Class::Separator => units.push(c.to_string()),
            _ => runs.push((k, vec![c.to_string()])),
        }
    }

    // 2. terms per run
    let mut terms: Vec<String> = Vec::new();
    let mut push = |t: String| {
        if !t.is_empty() && !terms.contains(&t) {
            terms.push(t);
        }
    };
    for (k, units) in runs {
        match k {
            Class::Word => {
                let w = units
                    .concat()
                    .trim_matches(|c| c == '-' || c == '_')
                    .to_lowercase();
                if w.chars().filter(|c| !is_combining(*c)).count() >= 3
                    && !EN_STOP.contains(&w.as_str())
                {
                    push(w);
                }
            }
            Class::Han => bigrams(&units)
                .into_iter()
                .filter(|b| keep_bigram(b))
                .for_each(&mut push),
            Class::Katakana if units.len() >= 2 => push(units.concat()),
            Class::Hangul | Class::Sea => bigrams(&units).into_iter().for_each(&mut push),
            _ => {}
        }
    }

    // 3. too many: keep the longest (they carry the most meaning), in their original order
    if terms.len() > MAX_TERMS {
        let mut by_len: Vec<(usize, &String)> = terms.iter().enumerate().collect();
        by_len.sort_by(|a, b| {
            b.1.chars()
                .count()
                .cmp(&a.1.chars().count())
                .then(a.0.cmp(&b.0))
        });
        let mut keep: Vec<usize> = by_len.into_iter().take(MAX_TERMS).map(|(i, _)| i).collect();
        keep.sort();
        terms = keep.into_iter().map(|i| terms[i].clone()).collect();
    }
    terms
}

/// One note the task's terms found.
#[derive(Debug, Clone)]
pub struct Match {
    pub hit: Hit,
    /// The terms the note contains, the rarest first.
    pub terms: Vec<String>,
    /// For each of `terms`, its weight ([`weight`]) times the note's share of it ([`share`]),
    /// and the weight once more when the title has it.
    score: f64,
}

/// A note one link away from the matches that is not one of them.
#[derive(Debug, Clone)]
pub struct Linked {
    pub id: String,
    pub scope: Scope,
    pub note_type: String,
    pub status: String,
    pub title: String,
    /// The ids of the matches it is linked with, best match first.
    pub via: Vec<String>,
}

/// What `memory_context` found for a task. [`Recall::render`] is the text the CLI and the MCP
/// server answer with.
#[derive(Debug, Clone)]
pub struct Recall {
    pub task: String,
    /// What the task was searched for. Empty when it held nothing to search for.
    pub terms: Vec<String>,
    /// The parts of the task no note contains, as the task wrote them ([`unmatched_parts`]). Empty
    /// too when that is unknown, because a vault could not be searched or the search stopped at
    /// its limit.
    pub unmatched: Vec<String>,
    /// The best matches, best first.
    pub notes: Vec<Match>,
    /// Notes linked with the best matches, the one linked with the most of them first.
    pub linked: Vec<Linked>,
    /// The vault's map of content, when it has the task's terms. It is never one of `notes`: it
    /// would be there for every task.
    pub index: Option<Hit>,
    /// Open findings about the matches.
    pub findings: Vec<Hit>,
    /// What could not be searched or listed.
    pub problems: Vec<String>,
}

fn scope_of(hit: &Hit) -> Scope {
    if hit.scope == "project" {
        Scope::Project
    } else {
        Scope::Global
    }
}

/// How often `text` has `term`. A word of a script written with spaces counts only where a word
/// starts, so `rag` is not in `storage` and `token` is in `tokens` and `rotate_token`. A bigram
/// counts anywhere, because its script does not mark where a word starts.
fn count(text: &str, term: &str) -> usize {
    if term.chars().next().map(class) != Some(Class::Word) {
        return text.matches(term).count();
    }
    text.match_indices(term)
        .filter(|(at, _)| {
            text[..*at]
                .chars()
                .next_back()
                .is_none_or(|c| class(c) != Class::Word || c == '-' || c == '_')
        })
        .count()
}

/// One note the search handed over, as the ranking reads it.
struct Candidate {
    hit: Hit,
    /// The terms it has, by their place in the task's terms: how often, and whether the title
    /// has it too.
    terms: Vec<(usize, usize, bool)>,
    /// The note's length in characters.
    len: usize,
}

fn candidate(store: &Store, hit: Hit, terms: &[String]) -> Candidate {
    let text = std::fs::read_to_string(store.vault_for(scope_of(&hit)).join(&hit.path))
        .unwrap_or_default()
        .to_lowercase();
    let title = hit.title.to_lowercase();
    Candidate {
        terms: terms
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                let in_title = count(&title, t) > 0;
                let times = count(&text, t).max(usize::from(in_title));
                (times > 0).then_some((i, times, in_title))
            })
            .collect(),
        len: text.chars().count(),
        hit,
    }
}

/// What a term is worth that `with` of the `of` notes the task found have. A term most of them
/// have (`does`, `not`) says little about which note the task is about.
fn weight(with: usize, of: usize) -> f64 {
    (1.0 + of as f64 / with.max(1) as f64).ln()
}

/// How much of a term's weight a note gets for having it `times` times: more for more, but each
/// time adds less, and a note longer than the others (`len` to `average`) needs more of them. A
/// long note has a little of every word. This is the term-frequency part of BM25, with its usual
/// constants.
fn share(times: usize, len: usize, average: f64) -> f64 {
    const K1: f64 = 1.2;
    const B: f64 = 0.75;
    let times = times as f64;
    times * (K1 + 1.0) / (times + K1 * (1.0 - B + B * len as f64 / average.max(1.0)))
}

/// The parts of a task that no note contains: the terms for which `found` is false, as the
/// lowercased task wrote them. Bigrams that overlap are one part again (`備份`, `份保`, `保留` →
/// `備份保留`), and a character that a found term covers is left out, because a bigram across
/// two words is no word (`搜尋架構` with `搜尋` found → `架構`).
fn unmatched_parts(task: &str, terms: &[String], found: impl Fn(&String) -> bool) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq, PartialOrd)]
    enum In {
        Nothing,
        Unmatched,
        Found,
    }
    let text: Vec<char> = task.to_lowercase().chars().collect();
    let mut cover = vec![In::Nothing; text.len()];
    for term in terms {
        let t: Vec<char> = term.chars().collect();
        let kind = if found(term) {
            In::Found
        } else {
            In::Unmatched
        };
        for (at, window) in text.windows(t.len().max(1)).enumerate() {
            if window == t {
                for c in &mut cover[at..at + t.len()] {
                    if *c < kind {
                        *c = kind;
                    }
                }
            }
        }
    }
    let mut parts: Vec<String> = Vec::new();
    let mut part = String::new();
    for (c, kind) in text.iter().zip(cover).chain([(&' ', In::Nothing)]) {
        if kind == In::Unmatched {
            part.push(*c);
            continue;
        }
        // one character is what a found bigram left of its neighbour
        if part.chars().count() > 1 && !parts.contains(&part) {
            parts.push(std::mem::take(&mut part));
        }
        part.clear();
    }
    parts
}

/// The notes one link away from the best of `notes` that are not among `notes`. A problem with
/// the backend is returned as text, with what was found before it.
fn linked(store: &Store, notes: &[Match]) -> (Vec<Linked>, Option<String>) {
    let shown = |scope: Scope, rel: &str| {
        notes
            .iter()
            .any(|m| scope_of(&m.hit) == scope && m.hit.path == rel)
    };
    let mut out: Vec<(String, Linked)> = Vec::new();
    let mut problem = None;
    for m in notes.iter().filter(|m| m.hit.path != INDEX).take(FOLLOWED) {
        let scope = scope_of(&m.hit);
        let neighbours = match store.neighbours(scope, &m.hit.path) {
            Ok(n) => n,
            Err(e) => {
                problem = Some(format!("linked notes not listed: {e:#}"));
                break;
            }
        };
        for rel in neighbours {
            if rel == INDEX
                || rel.starts_with("Templates/")
                || rel.starts_with("_system/")
                || shown(scope, &rel)
            {
                continue;
            }
            // a file that is not a note has no entry
            let Some(meta) = store.meta(scope, &rel) else {
                continue;
            };
            if matches!(meta.status.as_str(), "archived" | "superseded")
                || meta.note_type == "finding"
            {
                continue;
            }
            let key = format!("{}:{rel}", scope.as_str());
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some((_, l)) => l.via.push(m.hit.id.clone()),
                None => out.push((
                    key,
                    Linked {
                        id: meta.id.clone(),
                        scope,
                        note_type: meta.note_type.clone(),
                        status: meta.status.clone(),
                        title: meta.title.clone(),
                        via: vec![m.hit.id.clone()],
                    },
                )),
            }
        }
    }
    let mut out: Vec<Linked> = out.into_iter().map(|(_, l)| l).collect();
    // stable: among equals, the one linked with the better match stays first
    out.sort_by_key(|l| std::cmp::Reverse(l.via.len()));
    out.truncate(MAX_LINKED);
    (out, problem)
}

/// Search both vaults for a task's terms and rank what is found: at most `limit` notes.
pub fn recall(store: &mut Store, task: &str, limit: usize) -> Result<Recall> {
    let mut recall = Recall {
        task: task.trim().to_string(),
        terms: task_terms(task),
        unmatched: Vec::new(),
        notes: Vec::new(),
        linked: Vec::new(),
        index: None,
        findings: Vec::new(),
        problems: Vec::new(),
    };
    if recall.terms.is_empty() {
        return Ok(recall);
    }
    let terms = &recall.terms;
    // Bare words: Obsidian finds a bare word anywhere, and a quoted one in few notes that have
    // it only as part of a longer word. `"token"` would leave out a note that says `tokens`
    // before the ranking sees it, and the built-in search would not.
    let query = terms.join(" OR ");
    let (hits, problems) = store.search(&query, None, None, CANDIDATES)?;
    let complete = problems.is_empty() && hits.len() < CANDIDATES;
    // The search matches a word anywhere, so it can hand over a note that has none of the terms
    // the way `count` counts them.
    let mut found: Vec<Candidate> = hits
        .into_iter()
        .map(|hit| candidate(store, hit, terms))
        .filter(|c| !c.terms.is_empty())
        .collect();
    let has = |c: &Candidate, term: usize| c.terms.iter().any(|(i, ..)| *i == term);
    if complete {
        recall.unmatched = unmatched_parts(task, terms, |t| {
            let at = terms.iter().position(|x| x == t);
            at.is_some_and(|at| found.iter().any(|c| has(c, at)))
        });
    }
    // The map of content links to every note, so it has most terms of any task.
    if let Some(at) = found.iter().position(|c| c.hit.path == INDEX) {
        recall.index = Some(found.remove(at).hit);
    }
    let weights: Vec<f64> = (0..terms.len())
        .map(|i| weight(found.iter().filter(|c| has(c, i)).count(), found.len()))
        .collect();
    let average = found.iter().map(|c| c.len).sum::<usize>() as f64 / found.len().max(1) as f64;
    let mut ranked: Vec<Match> = found
        .into_iter()
        .map(|mut c| {
            c.terms
                .sort_by(|a, b| weights[b.0].total_cmp(&weights[a.0]));
            Match {
                score: c
                    .terms
                    .iter()
                    .map(|(i, times, in_title)| {
                        weights[*i]
                            * (share(*times, c.len, average) + f64::from(u8::from(*in_title)))
                    })
                    .sum(),
                terms: c.terms.iter().map(|(i, ..)| terms[*i].clone()).collect(),
                hit: c.hit,
            }
        })
        .collect();
    // Equal scores: the project's notes first, then by path, so that both backends give one order.
    ranked.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| (a.hit.scope != "project").cmp(&(b.hit.scope != "project")))
            .then_with(|| a.hit.path.cmp(&b.hit.path))
    });
    // findings are listed separately, against the notes they are about
    let (findings, mut notes): (Vec<_>, Vec<_>) = ranked
        .into_iter()
        .partition(|m| m.hit.note_type == "finding");
    notes.truncate(limit);

    let shown: Vec<String> = notes
        .iter()
        .map(|m| format!("{}:{}", m.hit.scope, m.hit.path))
        .collect();
    recall.findings = findings
        .into_iter()
        .map(|m| m.hit)
        .filter(|h| h.status == "open" || h.status == "accepted")
        .filter(|h| {
            let target = std::fs::read_to_string(store.vault_for(scope_of(h)).join(&h.path))
                .ok()
                .and_then(|s| Note::parse(&s).ok())
                .and_then(|n| {
                    n.fm.extra
                        .get("target")
                        .and_then(|v| v.as_str())
                        .map(|t| format!("{}:{t}", h.scope))
                });
            target.is_some_and(|t| shown.contains(&t))
        })
        .collect();
    let (linked, problem) = linked(store, &notes);
    recall.linked = linked;
    recall.notes = notes;
    recall.problems = problems;
    recall.problems.extend(problem);
    Ok(recall)
}

impl Recall {
    /// The answer as text, cut to about `max_chars`.
    pub fn render(&self, max_chars: usize) -> String {
        if self.terms.is_empty() {
            return "No searchable terms in the task description; use memory_search with specific words."
                .into();
        }
        let mut out = format!(
            "Memory relevant to: {}\n(terms: {})\n",
            self.task,
            self.terms.join(", ")
        );
        if !self.unmatched.is_empty() {
            out.push_str(&format!(
                "(no note contains: {}. If they matter, search again with other words or in \
                 another language.)\n",
                self.unmatched.join(", ")
            ));
        }
        if self.notes.is_empty() {
            out.push_str("\nNo matching notes.\n");
        }
        let fits = |out: &str, line: &str| out.chars().count() + line.chars().count() <= max_chars;
        let unreviewed = |status: &str| {
            if status == "draft" {
                ", draft: unreviewed"
            } else {
                ""
            }
        };
        for m in &self.notes {
            let h = &m.hit;
            let line = format!(
                "\n- [[{}]] ({}/{}{}, matches: {}) {}\n  {}\n",
                h.id,
                h.scope,
                h.note_type,
                unreviewed(&h.status),
                m.terms.join(", "),
                h.title,
                h.snippet
            );
            if !fits(&out, &line) {
                out.push_str("\n…(more matches; narrow with memory_search)\n");
                break;
            }
            out.push_str(&line);
        }
        let mut heading = "\nOne link away from these notes (read with memory_get):\n";
        for l in &self.linked {
            let via: Vec<String> = l.via.iter().map(|id| format!("[[{id}]]")).collect();
            let line = format!(
                "{heading}- [[{}]] ({}/{}{}) {}, linked with {}\n",
                l.id,
                l.scope.as_str(),
                l.note_type,
                unreviewed(&l.status),
                l.title,
                via.join(", ")
            );
            if !fits(&out, &line) {
                break;
            }
            out.push_str(&line);
            heading = "";
        }
        if let Some(h) = &self.index {
            out.push_str(&format!(
                "\nNot listed above: [[{}]], the map of every note in the {} vault.\n",
                h.id, h.scope
            ));
        }
        if !self.findings.is_empty() {
            out.push_str("\nOpen findings on these notes (check before trusting them):\n");
            for h in &self.findings {
                out.push_str(&format!("- [[{}]] {}\n", h.id, h.title));
            }
        }
        for p in &self.problems {
            out.push_str(&format!("\nNote: {p}\n"));
        }
        out
    }
}

pub fn context(store: &mut Store, task: &str, limit: usize, max_chars: usize) -> Result<String> {
    Ok(recall(store, task, limit)?.render(max_chars))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_from_mixed_text() {
        assert_eq!(
            task_terms("Fix the rotate_token bug in auth"),
            vec!["rotate_token", "bug", "auth"]
        );
        assert_eq!(task_terms("幫我修 JWT 輪替"), vec!["jwt", "輪替"]); // 幫我 / 我修 carry no topic
        assert_eq!(task_terms("code-drift 規則"), vec!["code-drift", "規則"]);
        assert_eq!(task_terms("登入流程"), vec!["登入", "入流", "流程"]);
        assert!(task_terms("how to do it?").is_empty());
    }

    #[test]
    fn terms_in_other_scripts() {
        let t = |s: &str| task_terms(s);
        assert_eq!(
            t("幫我修改 refresh token 的輪替"),
            vec!["修改", "refresh", "token", "輪替"]
        );
        // Katakana loanwords whole, hiragana particles dropped, kanji bigrams
        assert_eq!(
            t("トークンのローテーションを変更する"),
            vec!["トークン", "ローテーション", "変更"]
        );
        assert_eq!(
            t("토큰 교체 방식을 바꾸기"),
            vec!["토큰", "교체", "방식", "식을", "바꾸", "꾸기"]
        );
        // accented Latin letters stay inside the word
        assert_eq!(
            t("Änderung der Token-Rotation"),
            vec!["änderung", "der", "token-rotation"]
        );
        assert_eq!(
            t("Modifier la rotation des jetons"),
            vec!["modifier", "rotation", "des", "jetons"]
        );
        assert_eq!(
            t("изменить ротацию токенов"),
            vec!["изменить", "ротацию", "токенов"]
        );
        assert_eq!(t("αλλαγή περιστροφής"), vec!["αλλαγή", "περιστροφής"]);
        assert_eq!(t("تغيير تدوير الرمز"), vec!["تغيير", "تدوير", "الرمز"]);
        assert_eq!(t("שינוי סבב אסימונים"), vec!["שינוי", "סבב", "אסימונים"]);
        // Devanagari: vowel signs and viramas belong to the word; the danda separates
        assert_eq!(t("टोकन रोटेशन बदलें।"), vec!["टोकन", "रोटेशन", "बदलें"]);
        // decomposed accents (NFD) stay attached
        assert_eq!(t("cafe\u{301} menu"), vec!["cafe\u{301}", "menu"]);
        // Thai has no spaces: cluster bigrams, every one a real substring of the text
        let thai = "เปลี่ยนการหมุนเวียนโทเค็น";
        let th = t(thai);
        assert!(
            !th.is_empty() && th.iter().all(|x| thai.contains(x.as_str())),
            "{th:?}"
        );
        assert!(
            th.iter().all(|x| !x.starts_with(is_sea_mark)),
            "no term starts with a mark: {th:?}"
        );
    }

    #[test]
    fn unmatched_parts_read_like_the_task() {
        let parts = |task: &str, found: &[&str]| {
            unmatched_parts(task, &task_terms(task), |t| found.contains(&t.as_str()))
        };
        // overlapping bigrams are one part again
        assert_eq!(parts("調整舊備份保留多久", &[]), ["調整舊備份保留多久"]);
        // a bigram across a found word and the next one is not reported
        assert_eq!(parts("加強搜尋架構", &["搜尋"]), ["加強", "架構"]);
        assert_eq!(
            parts("Agentic RAG retrieval 的搜尋", &["rag", "搜尋"]),
            ["agentic", "retrieval"]
        );
        assert!(parts("rotate the signing key", &["rotate", "signing", "key"]).is_empty());
        // the same part twice is reported once
        assert_eq!(parts("備份 和 備份", &[]), ["備份"]);
    }

    /// A project with these notes in its vault, searched without Obsidian.
    fn store(name: &str, notes: &[(&str, &str)]) -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("obmem-context-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("global")).unwrap();
        std::fs::write(dir.join(".obmem.toml"), "").unwrap();
        for (rel, content) in notes {
            let p = dir.join("kb").join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        let cfg = crate::config::Config {
            project_root: dir.clone(),
            project_name: "t".into(),
            project_vault: dir.join("kb"),
            global_vault: dir.join("global"),
            secrets: Default::default(),
            audit: Default::default(),
            auto_approve: false,
            device: "test".into(),
        };
        let store = Store::open_with(cfg, crate::obsidian::Backend::Headless).unwrap();
        (store, dir)
    }

    #[test]
    fn recall_says_where_to_go_next() {
        let note = |ty: &str, status: &str, body: &str| {
            format!("---\ntype: {ty}\nstatus: {status}\n---\n{body}\n")
        };
        let (mut store, dir) = store(
            "next",
            &[
                (
                    "Index.md",
                    &note(
                        "concept",
                        "active",
                        "[[jwt-rotation]] [[secret-store]] [[billing]]",
                    ),
                ),
                (
                    "Decisions/jwt-rotation.md",
                    &note(
                        "decision",
                        "active",
                        "Rotate the signing key. See [[secret-store]], [[old-rotation]] and [[gone]].",
                    ),
                ),
                (
                    "Knowledge/secret-store.md",
                    &note("fact", "draft", "Where the secret lives."),
                ),
                (
                    "Knowledge/audit-log.md",
                    &note(
                        "fact",
                        "active",
                        "Written on every change. See [[jwt-rotation]].",
                    ),
                ),
                (
                    "Archive/old-rotation.md",
                    &note("decision", "superseded", "Nothing was changed."),
                ),
                (
                    "Knowledge/billing.md",
                    &note("fact", "active", "Invoices go out monthly."),
                ),
            ],
        );
        let r = recall(&mut store, "rotate the signing key 的流程", 8).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let ids: Vec<&str> = r.notes.iter().map(|m| m.hit.id.as_str()).collect();
        assert_eq!(ids, ["jwt-rotation"]);
        assert_eq!(r.notes[0].terms, ["rotate", "signing", "key"]);
        assert_eq!(r.unmatched, ["流程"]);
        // one link away, in either direction: not the index, a superseded note or a missing one
        let linked: Vec<(&str, &[String])> = r
            .linked
            .iter()
            .map(|l| (l.id.as_str(), l.via.as_slice()))
            .collect();
        assert_eq!(
            linked,
            [
                ("secret-store", &["jwt-rotation".to_string()][..]),
                ("audit-log", &["jwt-rotation".to_string()][..]),
            ]
        );
        let text = r.render(4000);
        assert!(
            text.contains("(no note contains: 流程. ")
                && text.contains("- [[jwt-rotation]] (project/decision, matches: rotate, signing, key) ")
                && text.contains(
                    "- [[secret-store]] (project/fact, draft: unreviewed) secret-store, linked with [[jwt-rotation]]\n"
                ),
            "{text}"
        );
        // a task with nothing to search for
        let r = recall(&mut store, "how to do it?", 8).unwrap();
        assert!(r.render(4000).starts_with("No searchable terms"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_word_counts_where_a_word_starts() {
        assert_eq!(count("storage of paragraphs", "rag"), 0);
        assert_eq!(count("tokens, rotate_token and refresh-token", "token"), 3);
        assert_eq!(count("rag: the rag pipeline", "rag"), 2);
        // a word after a script without spaces starts a word; a bigram counts anywhere
        assert_eq!(count("使用git的流程", "git"), 1);
        assert_eq!(count("備份與備份", "備份"), 2);
    }

    #[test]
    fn weak_matches_do_not_outrank_the_note_the_task_is_about() {
        let note = |body: &str| format!("---\ntype: fact\nstatus: active\n---\n{body}\n");
        let long = format!(
            "Storage does compress paragraphs. {}",
            "Nothing else. ".repeat(40)
        );
        let (mut store, dir) = store(
            "weak",
            &[
                ("Index.md", &note("[[rag]] [[storage]] [[b]] [[a]]")),
                (
                    "Knowledge/rag.md",
                    &note("RAG does not fit a vault this small."),
                ),
                ("Knowledge/storage.md", &note(&long)),
                ("Knowledge/b.md", &note("It does.")),
                ("Knowledge/a.md", &note("It does.")),
            ],
        );
        let r = recall(&mut store, "does obmem use RAG", 8).unwrap();
        let ids: Vec<&str> = r.notes.iter().map(|m| m.hit.id.as_str()).collect();
        // `rag` is not in `storage`, a long note that says `does` once gets less for it than a
        // short one, and equal notes come by path
        assert_eq!(ids, ["rag", "a", "b", "storage"]);
        // the rarest term first
        assert_eq!(r.notes[0].terms, ["rag", "does"]);
        assert_eq!(r.notes[3].terms, ["does"]);
        assert_eq!(r.unmatched, ["obmem"]);
        // the map of content has every note's name, and is named without taking a place
        assert_eq!(r.index.as_ref().map(|h| h.id.as_str()), Some("Index"));
        assert!(
            r.render(4000).contains(
                "\nNot listed above: [[Index]], the map of every note in the project vault.\n"
            ),
            "{}",
            r.render(4000)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn too_many_terms_keeps_the_longest() {
        let long = "authentication middleware refactor plus rate limiting and retries for webhooks, \
                    idempotency keys, structured logging, metrics, tracing spans, error budgets, \
                    canary deploys, feature flags, rollback";
        let t = task_terms(long);
        assert_eq!(t.len(), MAX_TERMS);
        assert!(
            t.contains(&"authentication".to_string()) && !t.contains(&"plus".to_string()),
            "{t:?}"
        );
        let pos = |w: &str| t.iter().position(|x| x == w).unwrap();
        assert!(
            pos("authentication") < pos("middleware"),
            "original order kept"
        );
    }
}
