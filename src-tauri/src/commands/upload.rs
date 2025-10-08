use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use crate::utils::cancel_current_operation;
use crate::audio_toolkit::audio::resampler::FrameResampler;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, State, Emitter};
use rodio::{Decoder, Source};
use std::fs::File;
use std::io::BufReader;
use base64::Engine as _;
use base64::engine::general_purpose;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

fn decode_audio_with_symphonia(file_path: &str) -> Result<(Vec<f32>, u32), String> {
    // Create a hint to help the format registry guess what format reader is appropriate.
    let mut hint = Hint::new();

    // Provide the file extension as a hint.
    if let Some(extension) = Path::new(file_path).extension() {
        if let Some(extension_str) = extension.to_str() {
            hint.with_extension(extension_str);
        }
    }

    // Create the media source stream.
    let file = File::open(file_path)
        .map_err(|e| format!("Dosya açma hatası: {}", e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // Use the default options for format and metadata readers.
    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();

    // Probe the media source stream for a format.
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| format!("Format algılama hatası: {}", e))?;

    // Get the format reader.
    let mut format = probed.format;

    // Get the default track.
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or("Geçerli bir ses track'i bulunamadı")?;

    let track_id = track.id;

    // Create a decoder for the track.
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Decoder oluşturma hatası: {}", e))?;

    // Get the sample rate
    let sample_rate = track.codec_params.sample_rate
        .ok_or("Sample rate bilgisi bulunamadı")?;

    let mut audio_samples = Vec::new();

    // Decode the track.
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::ResetRequired) => {
                // The track list has been changed. Re-examine it and create a new set of decoders,
                // then restart the decode loop. This is an advanced feature and it is not
                // unreasonable to consider this "the end." As of now, the only usage of this is
                // for chained OGG physical streams.
                unimplemented!();
            }
            Err(symphonia::core::errors::Error::IoError(_)) => {
                // End of stream
                break;
            }
            Err(err) => {
                return Err(format!("Paket okuma hatası: {}", err));
            }
        };

        // If the packet does not belong to the selected track, skip it.
        if packet.track_id() != track_id {
            continue;
        }

        // Decode the packet into audio samples.
        match decoder.decode(&packet) {
            Ok(decoded) => {
                // Convert the decoded audio to f32 samples
                let mut sample_buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                sample_buf.copy_interleaved_ref(decoded);
                audio_samples.extend_from_slice(sample_buf.samples());
            }
            Err(symphonia::core::errors::Error::IoError(_)) => {
                // The packet failed to decode due to an IO error, but we can still try to decode
                // the next packet.
                continue;
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => {
                // The packet failed to decode due to invalid data, but we can still try to decode
                // the next packet.
                continue;
            }
            Err(err) => {
                return Err(format!("Ses kod çözme hatası: {}", err));
            }
        }
    }

    Ok((audio_samples, sample_rate))
}

#[tauri::command]
pub async fn transcribe_uploaded_audio(
    app: AppHandle,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    history_manager: State<'_, Arc<HistoryManager>>,
    file_data: String, // Base64 encoded file data
    file_name: String,
    _model_id: String,
    save_to_history: bool,
) -> Result<String, String> {
    // Cancel any ongoing operations
    cancel_current_operation(&app);

    println!("Starting transcription for file: {}", file_name);

    // Decode base64 file data
    println!("Decoding base64 data...");
    let file_bytes = general_purpose::STANDARD.decode(&file_data)
        .map_err(|e| format!("Base64 decode hatası: {}", e))?;

    println!("Decoded {} bytes", file_bytes.len());

    // Create temporary file
    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("handy_upload_{}", file_name));
    println!("Writing to temp file: {:?}", temp_file_path);
    std::fs::write(&temp_file_path, file_bytes)
        .map_err(|e| format!("Geçici dosya yazma hatası: {}", e))?;

    let file_path_str = temp_file_path.to_string_lossy().to_string();

    // Validate file exists
    if !Path::new(&file_path_str).exists() {
        return Err("Geçici dosya bulunamadı".to_string());
    }

    println!("Temp file created successfully: {}", file_path_str);

    // Get managers
    let transcription_manager = transcription_manager.inner().clone();
    let history_manager = history_manager.inner().clone();

    // Load and process audio file
    println!("Opening audio file...");
    let file_extension = Path::new(&file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    let (mut audio_samples, sample_rate) = if file_extension == "m4a" || file_extension == "aac" || file_extension == "mp4" {
        println!("Using symphonia for M4A/AAC decoding...");
        decode_audio_with_symphonia(&file_path_str)?
    } else {
        println!("Using rodio for decoding...");
        let file = File::open(&file_path_str)
            .map_err(|e| format!("Dosya açma hatası: {}", e))?;
        println!("Creating decoder...");
        let decoder = Decoder::new(BufReader::new(file))
            .map_err(|e| format!("Ses kod çözme hatası: {}", e))?;

        // Get sample rate before consuming decoder
        let sample_rate = decoder.sample_rate();
        println!("Sample rate: {}", sample_rate);

        // Convert to f32 samples
        println!("Converting to f32 samples...");
        let audio_samples: Vec<f32> = decoder.collect();
        (audio_samples, sample_rate)
    };

    println!("Decoded {} samples at {} Hz", audio_samples.len(), sample_rate);

    // Resample to 16kHz if needed
    if sample_rate != 16000 {
        println!("Resampling from {} Hz to 16000 Hz...", sample_rate);
        let mut resampled_samples = Vec::new();
        let mut resampler = FrameResampler::new(sample_rate as usize, 16000, std::time::Duration::from_millis(30));

        resampler.push(&audio_samples, |frame| {
            resampled_samples.extend_from_slice(frame);
        });

        audio_samples = resampled_samples;
        println!("Resampled to {} samples", audio_samples.len());
    }

    // Apply VAD to filter silence
    // TODO: Implement proper VAD with model loading
    let filtered_samples = audio_samples.clone(); // For now, skip VAD filtering

    // Transcribe
    println!("Starting transcription with {} samples...", filtered_samples.len());
    let transcription = transcription_manager.transcribe(filtered_samples.clone())
        .map_err(|e| format!("Transkripsiyon hatası: {:?}", e))?;
    println!("Transcription completed, length: {}", transcription.len());

    // Save to history if requested
    if save_to_history {
        history_manager
            .save_transcription(audio_samples, transcription.clone())
            .await
            .map_err(|e| format!("Geçmişe kaydetme hatası: {}", e))?;

        // Emit history updated event
        app.emit("history-updated", ())
            .map_err(|e| format!("Event gönderme hatası: {}", e))?;
    }

    Ok(transcription)
}