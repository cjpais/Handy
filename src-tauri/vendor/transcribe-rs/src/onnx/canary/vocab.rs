use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::TranscribeError;

pub struct Vocab {
    token_to_id_map: HashMap<String, i64>,
    id_to_token_map: HashMap<i64, String>,
    eos_id: i64,
    size: usize,
}

impl Vocab {
    pub fn load(path: &Path) -> Result<Self, TranscribeError> {
        let content = fs::read_to_string(path)
            .map_err(|e| TranscribeError::Config(format!("Failed to read vocab file: {e}")))?;

        let mut token_to_id_map = HashMap::new();
        let mut id_to_token_map = HashMap::new();

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let last_space = line.rfind(' ').ok_or_else(|| {
                TranscribeError::Config(format!(
                    "Invalid vocab line {}: missing space separator",
                    line_num + 1
                ))
            })?;

            let token = &line[..last_space];
            let id_str = &line[last_space + 1..];

            let id: i64 = id_str.parse().map_err(|e| {
                TranscribeError::Config(format!("Invalid token ID on line {}: {e}", line_num + 1))
            })?;

            token_to_id_map.insert(token.to_string(), id);
            id_to_token_map.insert(id, token.to_string());
        }

        let eos_id = *token_to_id_map.get("<|endoftext|>").ok_or_else(|| {
            TranscribeError::Config("Vocabulary missing required <|endoftext|> token".to_string())
        })?;

        let size = token_to_id_map.len();

        log::info!(
            "Loaded vocabulary with {} tokens from {}",
            size,
            path.display()
        );

        Ok(Self {
            token_to_id_map,
            id_to_token_map,
            eos_id,
            size,
        })
    }

    pub fn token_to_id(&self, token: &str) -> Option<i64> {
        self.token_to_id_map.get(token).copied()
    }

    pub fn id_to_token(&self, id: i64) -> Option<&str> {
        self.id_to_token_map.get(&id).map(|s| s.as_str())
    }

    pub fn eos_token_id(&self) -> i64 {
        self.eos_id
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn build_prompt(
        &self,
        src_lang: &str,
        tgt_lang: &str,
        use_pnc: bool,
        use_itn: bool,
    ) -> Result<Vec<i64>, TranscribeError> {
        let pnc_token = if use_pnc { "<|pnc|>" } else { "<|nopnc|>" };
        let itn_token = if use_itn { "<|itn|>" } else { "<|noitn|>" };

        let tokens = [
            "<|startofcontext|>".to_string(),
            "<|startoftranscript|>".to_string(),
            "<|emo:undefined|>".to_string(),
            format!("<|{src_lang}|>"),
            format!("<|{tgt_lang}|>"),
            pnc_token.to_string(),
            itn_token.to_string(),
            "<|notimestamp|>".to_string(),
            "<|nodiarize|>".to_string(),
        ];

        let mut ids = Vec::with_capacity(tokens.len());
        for token in &tokens {
            let id = self.token_to_id(token).ok_or_else(|| {
                TranscribeError::Config(format!("Prompt token not found in vocabulary: {token}"))
            })?;
            ids.push(id);
        }

        log::debug!("Built prompt ({} tokens): {:?}", ids.len(), ids);
        Ok(ids)
    }

    pub fn decode_tokens(&self, token_ids: &[i64]) -> String {
        let mut pieces: Vec<String> = Vec::new();

        for &id in token_ids {
            if let Some(token) = self.id_to_token(id) {
                if token.starts_with("<|") {
                    continue;
                }
                let cleaned = token.replace('\u{2581}', " ");
                pieces.push(cleaned);
            }
        }

        let text = pieces.join("");
        text.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("canary_vocab_test_{name}_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_vocab_load_and_lookup() {
        let dir = create_temp_dir("load");
        let vocab_path = dir.join("vocab.txt");
        let mut f = fs::File::create(&vocab_path).unwrap();
        writeln!(f, "<|endoftext|> 3").unwrap();
        writeln!(f, "hello 10").unwrap();
        writeln!(f, "world 20").unwrap();

        let vocab = Vocab::load(&vocab_path).unwrap();

        assert_eq!(vocab.token_to_id("<|endoftext|>"), Some(3));
        assert_eq!(vocab.token_to_id("hello"), Some(10));
        assert_eq!(vocab.id_to_token(20), Some("world"));
        assert_eq!(vocab.eos_token_id(), 3);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_decode_tokens_filters_special() {
        let dir = create_temp_dir("decode");
        let vocab_path = dir.join("vocab.txt");
        let mut f = fs::File::create(&vocab_path).unwrap();
        writeln!(f, "<|endoftext|> 3").unwrap();
        writeln!(f, "<|startoftranscript|> 1").unwrap();
        writeln!(f, "\u{2581}Hello 10").unwrap();
        writeln!(f, "\u{2581}world 20").unwrap();

        let vocab = Vocab::load(&vocab_path).unwrap();
        let text = vocab.decode_tokens(&[1, 10, 20, 3]);

        assert_eq!(text, "Hello world");

        let _ = fs::remove_dir_all(&dir);
    }
}
