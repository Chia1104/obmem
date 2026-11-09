//! `memory_context(task)`: the notes most relevant to a task description, within a size budget.
//! Obsidian does the searching; this only turns free text into an OR query (Obsidian ANDs
//! words, and many scripts have no spaces) and ranks the hits, which Obsidian search does not.
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
//! Matching stays literal (Obsidian substring search): "rotation" does not find 輪替, and an
//! inflected form only finds itself.

use crate::catalog::Hit;
use crate::note::Note;
use crate::store::Store;
use anyhow::Result;

const MAX_TERMS: usize = 16;
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

/// Rank hits by how many distinct terms the note contains (title matches count double).
fn score(store: &Store, hit: &Hit, terms: &[String]) -> usize {
    let root = store.vault_for(if hit.scope == "project" {
        crate::note::Scope::Project
    } else {
        crate::note::Scope::Global
    });
    let text = std::fs::read_to_string(root.join(&hit.path))
        .unwrap_or_default()
        .to_lowercase();
    let title = hit.title.to_lowercase();
    terms
        .iter()
        .map(|t| {
            if title.contains(t.as_str()) {
                2
            } else {
                usize::from(text.contains(t.as_str()))
            }
        })
        .sum()
}

pub fn context(store: &mut Store, task: &str, limit: usize, max_chars: usize) -> Result<String> {
    let terms = task_terms(task);
    if terms.is_empty() {
        return Ok(
            "No searchable terms in the task description; use memory_search with specific words."
                .into(),
        );
    }
    let query = terms
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" OR ");
    let (hits, problems) = store.search(&query, None, None, 40)?;
    let mut ranked: Vec<(usize, Hit)> = hits
        .into_iter()
        .map(|h| (score(store, &h, &terms), h))
        .collect();
    ranked.sort_by_key(|a| std::cmp::Reverse(a.0));
    // findings are listed separately below, against the notes they are about
    let (findings, notes): (Vec<_>, Vec<_>) = ranked
        .into_iter()
        .partition(|(_, h)| h.note_type == "finding");

    let mut out = format!(
        "Memory relevant to: {}\n(terms: {})\n",
        task.trim(),
        terms.join(", ")
    );
    if notes.is_empty() {
        out.push_str("\nNo matching notes.\n");
    }
    for (s, h) in notes.iter().take(limit) {
        let trust = if h.status == "draft" {
            ", draft: unreviewed"
        } else {
            ""
        };
        let line = format!(
            "\n- [[{}]] ({}/{}{trust}, score {s}) {}\n  {}\n",
            h.id, h.scope, h.note_type, h.title, h.snippet
        );
        if out.chars().count() + line.chars().count() > max_chars {
            out.push_str("\n…(more matches; narrow with memory_search)\n");
            break;
        }
        out.push_str(&line);
    }
    let shown: Vec<String> = notes
        .iter()
        .take(limit)
        .map(|(_, h)| format!("{}:{}", h.scope, h.path))
        .collect();
    let related: Vec<&Hit> = findings
        .iter()
        .map(|(_, h)| h)
        .filter(|h| h.status == "open" || h.status == "accepted")
        .filter(|h| {
            let root = store.vault_for(if h.scope == "project" {
                crate::note::Scope::Project
            } else {
                crate::note::Scope::Global
            });
            let target = std::fs::read_to_string(root.join(&h.path))
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
    if !related.is_empty() {
        out.push_str("\nOpen findings on these notes (check before trusting them):\n");
        for h in related {
            out.push_str(&format!("- [[{}]] {}\n", h.id, h.title));
        }
    }
    for p in problems {
        out.push_str(&format!("\nNote: {p}\n"));
    }
    Ok(out)
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
