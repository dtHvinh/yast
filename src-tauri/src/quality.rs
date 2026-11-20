//! Judges how plausible OCR output is without knowing the right answer, so several preprocessing
//! variants can compete. Garbage from a bad variant tends to contain stray symbols, mixed-case
//! fragments and vowel-less letter runs; real text does not.

#[derive(Debug, Clone, Copy, Default)]
pub struct Quality {
    /// Letters and digits that sit inside plausible words or numbers.
    pub good: usize,
    /// Stray symbols and symbol-only tokens.
    pub junk: usize,
    /// All letters and digits seen.
    pub total: usize,
}

impl Quality {
    pub fn score(&self) -> i32 {
        self.good as i32 - 2 * self.junk as i32
    }

    /// Share of the recognised characters that look like real text (1.0 = all of it).
    pub fn ratio(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.good as f32 / (self.total + self.junk) as f32
        }
    }
}

const VOWELS: &str = "aeiouyàáâãäåæèéêëìíîïòóôõöøùúûüýÿ";

pub fn assess(text: &str) -> Quality {
    let mut q = Quality::default();
    for token in text.split_whitespace() {
        let core = token.trim_matches(|c: char| !c.is_alphanumeric());
        if core.is_empty() {
            q.junk += token.chars().count();
            continue;
        }
        let alnum = core.chars().filter(|c| c.is_alphanumeric()).count();
        let symbols = core.chars().filter(|c| !c.is_alphanumeric() && !"'’-".contains(*c)).count();
        q.total += alnum;
        q.junk += symbols;
        // Punctuation clinging to the outside of a word is normal; a pile of it is not.
        let outer = token.chars().count() - core.chars().count();
        q.junk += outer.saturating_sub(2);
        if symbols == 0 && is_wordlike(core) {
            q.good += alnum;
        }
    }
    q
}

fn is_wordlike(core: &str) -> bool {
    let has_digit = core.chars().any(|c| c.is_ascii_digit());
    let letters: Vec<char> = core.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return true; // a plain number
    }
    if has_digit {
        return false; // "5OO", "he11o": letters and digits mixed
    }
    if !letters.iter().all(|c| is_latin(*c)) {
        return true; // other scripts (CJK, Cyrillic, ...): no rules to apply
    }
    let n = letters.len();
    if n == 1 {
        return true;
    }
    let upper = letters.iter().filter(|c| c.is_uppercase()).count();
    let case_ok = upper == 0 || upper == n || (upper == 1 && letters[0].is_uppercase());
    if !case_ok {
        return false; // "wElcOme"
    }
    if n == 2 && upper == 2 {
        return true; // "HP", "MP", "OK"
    }
    letters.iter().any(|c| is_vowel(*c))
}

fn is_latin(c: char) -> bool {
    c.is_ascii_alphabetic() || ('\u{00C0}'..='\u{024F}').contains(&c)
}

fn is_vowel(c: char) -> bool {
    c.to_lowercase().any(|l| VOWELS.contains(l))
}

/// A recognised text together with its plausibility, one per preprocessing variant.
#[derive(Clone)]
pub struct Candidate {
    pub text: String,
    pub q: Quality,
}

impl Candidate {
    pub fn new(text: String) -> Self {
        let q = assess(&text);
        Self { text, q }
    }
}

fn letters_only(s: &str) -> Vec<char> {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + (ca != cb) as usize).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// 1.0 = same letters and digits (case, spacing and punctuation ignored), 0.0 = nothing in common.
pub fn similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (letters_only(a), letters_only(b));
    let longest = a.len().max(b.len());
    if longest == 0 {
        return 0.0;
    }
    (1.0 - edit_distance(&a, &b) as f32 / longest as f32).max(0.0)
}

/// Used only by the benchmark's early-exit comparison: two variants independently read the same clean
/// text. Returns the better of the agreeing pair.
#[cfg(test)]
pub fn confident(cands: &[Candidate]) -> Option<usize> {
    let clean = |c: &Candidate| c.q.good >= 4 && c.q.ratio() >= 0.9;
    let mut found: Option<usize> = None;
    for i in 0..cands.len() {
        for j in i + 1..cands.len() {
            if clean(&cands[i]) && clean(&cands[j]) && similarity(&cands[i].text, &cands[j].text) >= 0.9 {
                let better = if cands[j].q.score() > cands[i].q.score() { j } else { i };
                if found.map_or(true, |f| cands[better].q.score() > cands[f].q.score()) {
                    found = Some(better);
                }
            }
        }
    }
    found
}

/// Best candidate overall: plausibility plus support from the other variants that read the same thing.
pub fn choose(cands: &[Candidate]) -> usize {
    let mut best = (0usize, f32::MIN);
    for (i, c) in cands.iter().enumerate() {
        let support: f32 = cands
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, o)| similarity(&c.text, &o.text) * o.q.score().max(0) as f32 * 0.5)
            .sum();
        let total = c.q.score() as f32 + support;
        if total > best.1 {
            best = (i, total);
        }
    }
    best.0
}
