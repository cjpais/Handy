//! 音訊格式轉換
//!
//! 將 Base64 編碼的音訊資料轉換為 16kHz mono f32 samples

use base64::Engine;
use std::error::Error;
use std::fmt;
use std::io::Cursor;

/// 音訊轉換錯誤
#[derive(Debug)]
pub enum AudioConvertError {
    /// Base64 解碼錯誤
    Base64DecodeError(base64::DecodeError),
    /// WAV 解碼錯誤
    WavDecodeError(hound::Error),
    /// 不支援的格式
    UnsupportedFormat(String),
    /// 不支援的取樣格式
    UnsupportedSampleFormat,
}

impl fmt::Display for AudioConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioConvertError::Base64DecodeError(e) => write!(f, "Base64 decode error: {}", e),
            AudioConvertError::WavDecodeError(e) => write!(f, "WAV decode error: {}", e),
            AudioConvertError::UnsupportedFormat(format) => {
                write!(f, "Unsupported format: {}", format)
            }
            AudioConvertError::UnsupportedSampleFormat => write!(f, "Unsupported sample format"),
        }
    }
}

impl Error for AudioConvertError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AudioConvertError::Base64DecodeError(e) => Some(e),
            AudioConvertError::WavDecodeError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<base64::DecodeError> for AudioConvertError {
    fn from(err: base64::DecodeError) -> Self {
        AudioConvertError::Base64DecodeError(err)
    }
}

impl From<hound::Error> for AudioConvertError {
    fn from(err: hound::Error) -> Self {
        AudioConvertError::WavDecodeError(err)
    }
}

/// 音訊轉換結果
pub struct AudioConvertResult {
    /// 16kHz mono f32 samples (-1.0 ~ 1.0)
    pub samples: Vec<f32>,
    /// 音訊長度（毫秒）
    pub duration_ms: u64,
}

/// 從 Base64 字串轉換為 f32 samples
pub fn convert_from_base64(
    base64_data: &str,
    format: &str,
) -> Result<AudioConvertResult, AudioConvertError> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(base64_data)?;
    convert_from_bytes(&bytes, format)
}

/// 從原始 bytes 轉換為 f32 samples
pub fn convert_from_bytes(
    bytes: &[u8],
    format: &str,
) -> Result<AudioConvertResult, AudioConvertError> {
    match format.to_lowercase().as_str() {
        "wav" => decode_wav(bytes),
        "ogg" => {
            // OGG/Opus 支援將在後續實作
            Err(AudioConvertError::UnsupportedFormat(
                "ogg (not yet implemented)".to_string(),
            ))
        }
        _ => Err(AudioConvertError::UnsupportedFormat(format.to_string())),
    }
}

/// 解碼 WAV 檔案並重採樣至 16kHz mono
fn decode_wav(bytes: &[u8]) -> Result<AudioConvertResult, AudioConvertError> {
    let cursor = Cursor::new(bytes);
    let mut reader = hound::WavReader::new(cursor)?;
    let spec = reader.spec();

    // 讀取所有 samples 並轉換為 f32
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max_value = (1 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / max_value)
                .collect()
        }
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(|s| s.ok()).collect(),
    };

    // 轉換為 mono（如果是多聲道）
    let mono_samples = if spec.channels > 1 {
        samples
            .chunks(spec.channels as usize)
            .map(|chunk| chunk.iter().sum::<f32>() / chunk.len() as f32)
            .collect()
    } else {
        samples
    };

    // 重採樣至 16kHz（如果需要）
    let target_sample_rate = 16000;
    let final_samples = if spec.sample_rate != target_sample_rate {
        resample(&mono_samples, spec.sample_rate, target_sample_rate)
    } else {
        mono_samples
    };

    let duration_ms = (final_samples.len() as u64 * 1000) / target_sample_rate as u64;

    Ok(AudioConvertResult {
        samples: final_samples,
        duration_ms,
    })
}

/// 簡單線性重採樣
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio).ceil() as usize;

    (0..output_len)
        .map(|i| {
            let src_idx = i as f64 * ratio;
            let idx = src_idx.floor() as usize;
            let frac = src_idx.fract() as f32;

            if idx + 1 < samples.len() {
                samples[idx] * (1.0 - frac) + samples[idx + 1] * frac
            } else if idx < samples.len() {
                samples[idx]
            } else {
                0.0
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unsupported_format() {
        let result = convert_from_bytes(&[], "mp3");
        assert!(matches!(
            result,
            Err(AudioConvertError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn test_ogg_not_implemented() {
        let result = convert_from_bytes(&[], "ogg");
        assert!(matches!(
            result,
            Err(AudioConvertError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn test_resample() {
        // 48kHz -> 16kHz (3:1 ratio)
        let input: Vec<f32> = (0..48).map(|i| i as f32 / 48.0).collect();
        let output = resample(&input, 48000, 16000);
        assert_eq!(output.len(), 16);
    }

    #[test]
    fn test_resample_values() {
        // 簡單測試：確保重採樣後的值在合理範圍內
        let input: Vec<f32> = vec![0.0, 0.5, 1.0, 0.5, 0.0];
        let output = resample(&input, 48000, 16000);
        for sample in &output {
            assert!(*sample >= 0.0 && *sample <= 1.0);
        }
    }

    #[test]
    fn test_invalid_wav_data() {
        let result = convert_from_bytes(&[0, 1, 2, 3], "wav");
        assert!(matches!(result, Err(AudioConvertError::WavDecodeError(_))));
    }

    #[test]
    fn test_invalid_base64() {
        let result = convert_from_base64("not-valid-base64!!!", "wav");
        assert!(matches!(
            result,
            Err(AudioConvertError::Base64DecodeError(_))
        ));
    }
}
