use anyhow::Context;
use pastel_room::WordLists;
use std::fs;
use std::path::Path;

/// Load three text files into a `WordLists`. Each file is one word per line;
/// blank lines and lines starting with `#` are skipped.
pub fn load_from_dir(dir: &Path) -> anyhow::Result<WordLists> {
    let easy = read_words(&dir.join("words-easy.txt"))?;
    let medium = read_words(&dir.join("words-medium.txt"))?;
    let hard = read_words(&dir.join("words-hard.txt"))?;
    Ok(WordLists::new(easy, medium, hard))
}

fn read_words(path: &Path) -> anyhow::Result<Vec<String>> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect())
}
