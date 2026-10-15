//! Long-form chunking: long audio is decoded in chunks cut at pauses, because
//! most models' accuracy degrades past 30-40 s of audio in one run.
//!
//! Cuts come from per-frame voice activity probabilities. Every duration here
//! is in samples of 16 kHz audio, never in frames, so any detector's frame size
//! works with the same rules. The thresholds are Silero's.
//!
//! Ported from pi-voice's `chunking.ts`. The cut plan is the same; the Rust
//! version returns chunks for the caller to decode rather than decoding them.

use std::ops::Range;

use log::{debug, info};
use once_cell::sync::Lazy;
use regex::Regex;

use super::constants;

const RATE: usize = constants::WHISPER_SAMPLE_RATE as usize;

const TARGET_SECONDS: f64 = 30.0;
/// Audio up to this many samples is decoded in one piece; longer audio is cut.
pub const MAX_CHUNK_SAMPLES: usize = 40 * RATE;
const MIN_CHUNK_SAMPLES: usize = 5 * RATE;
/// Live, a cut is only final once this much audio past its window is scored:
/// then no cut it allows can leave a short rest, so live cuts match the plan
/// for the whole clip.
const LOOKAHEAD_SAMPLES: usize = MIN_CHUNK_SAMPLES;

// Silero's `get_speech_timestamps` defaults, as `VadSession::run` applies them.
const THRESHOLD: f32 = 0.5;
const NEG_THRESHOLD: f32 = THRESHOLD - 0.15;
const MIN_SPEECH_SAMPLES: usize = 250 * RATE / 1000;
const MIN_SILENCE_SAMPLES: usize = 100 * RATE / 1000;
const SPEECH_PAD_SAMPLES: usize = 30 * RATE / 1000;

/// Speech segments, in samples, from per-frame speech probabilities: Silero's
/// `get_speech_timestamps` without a maximum speech length. `samples` is the
/// length of the audio the probabilities score.
pub fn speech_segments(probs: &[f32], frame_samples: usize, samples: usize) -> Vec<Range<usize>> {
    let mut segments: Vec<Range<usize>> = Vec::new();
    let mut triggered = false;
    let mut start = 0;
    let mut temp_end: Option<usize> = None;
    for (i, &prob) in probs.iter().enumerate() {
        let at = i * frame_samples;
        if prob >= THRESHOLD {
            temp_end = None;
            if !triggered {
                triggered = true;
                start = at;
            }
            continue;
        }
        if prob < NEG_THRESHOLD && triggered {
            let end = *temp_end.get_or_insert(at);
            if at - end < MIN_SILENCE_SAMPLES {
                continue;
            }
            if end - start > MIN_SPEECH_SAMPLES {
                segments.push(start..end);
            }
            triggered = false;
            temp_end = None;
        }
    }
    if triggered && samples.saturating_sub(start) > MIN_SPEECH_SAMPLES {
        segments.push(start..samples);
    }

    for i in 0..segments.len() {
        if i == 0 {
            segments[0].start = segments[0].start.saturating_sub(SPEECH_PAD_SAMPLES);
        }
        if i + 1 == segments.len() {
            segments[i].end = (segments[i].end + SPEECH_PAD_SAMPLES).min(samples);
            continue;
        }
        let silence = segments[i + 1].start - segments[i].end;
        let pad = if silence < 2 * SPEECH_PAD_SAMPLES {
            silence / 2
        } else {
            SPEECH_PAD_SAMPLES
        };
        segments[i].end = (segments[i].end + pad).min(samples);
        segments[i + 1].start = segments[i + 1].start.saturating_sub(pad);
    }
    segments
}

/// A place a chunk may end: the middle of a pause.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutCandidate {
    pub cut: usize,
    pub gap_seconds: f64,
}

/// One candidate in the middle of each pause, lead-in and tail included.
pub fn cut_candidates(segments: &[Range<usize>], samples: usize) -> Vec<CutCandidate> {
    let mut candidates = Vec::new();
    let mut silence_start = 0;
    for segment in segments
        .iter()
        .cloned()
        .chain(std::iter::once(samples..samples))
    {
        if segment.start > silence_start {
            candidates.push(CutCandidate {
                cut: (silence_start + segment.start) / 2,
                gap_seconds: (segment.start - silence_start) as f64 / RATE as f64,
            });
        }
        silence_start = silence_start.max(segment.end);
    }
    candidates
}

/// Greedy, left to right from `from`: each cut is the best-scoring pause
/// 5-40 s after the previous one, preferring long pauses near 30 s and ones
/// that leave at least 5 s after them. With no pause, it is a hard cut at 40 s,
/// or halfway when that would leave under 5 s. Audio of 40 s or less is never
/// cut. `candidates` must be in order.
pub fn plan_cuts(candidates: &[CutCandidate], samples: usize, from: usize) -> Vec<usize> {
    let mut cuts = Vec::new();
    let mut start = from;
    // Each window starts after the last, so earlier candidates never return.
    let mut first = 0;
    while samples - start > MAX_CHUNK_SAMPLES {
        while first < candidates.len() && candidates[first].cut < start + MIN_CHUNK_SAMPLES {
            first += 1;
        }
        // A pause that leaves under 5 s is only used when no other pause will do.
        let mut best: Option<(usize, f64)> = None;
        let mut fallback: Option<(usize, f64)> = None;
        for candidate in candidates[first..]
            .iter()
            .take_while(|candidate| candidate.cut <= start + MAX_CHUNK_SAMPLES)
        {
            let position = (candidate.cut - start) as f64 / RATE as f64;
            let off_target = if position > TARGET_SECONDS {
                0.1 * (position - TARGET_SECONDS)
            } else {
                0.05 * (TARGET_SECONDS - position)
            };
            let score = candidate.gap_seconds.min(1.0) - off_target;
            if samples - candidate.cut >= MIN_CHUNK_SAMPLES
                && best.is_none_or(|(_, best)| score > best)
            {
                best = Some((candidate.cut, score));
            }
            if fallback.is_none_or(|(_, fallback)| score > fallback) {
                fallback = Some((candidate.cut, score));
            }
        }
        let hard_cut = if samples - start < MAX_CHUNK_SAMPLES + MIN_CHUNK_SAMPLES {
            start + (samples - start) / 2
        } else {
            start + MAX_CHUNK_SAMPLES
        };
        start = best.or(fallback).map_or(hard_cut, |(cut, _)| cut);
        cuts.push(start);
    }
    cuts
}

/// Chunks of a whole clip, as ranges covering all of it in order. A clip of
/// [`MAX_CHUNK_SAMPLES`] or less is one chunk.
pub fn plan_chunks(probs: &[f32], frame_samples: usize, samples: usize) -> Vec<Range<usize>> {
    // Scores past the clip would place speech, and so cuts, beyond its end.
    let probs = &probs[..probs.len().min(samples.div_ceil(frame_samples))];
    let segments = speech_segments(probs, frame_samples, samples);
    let cuts = plan_cuts(&cut_candidates(&segments, samples), samples, 0);
    let mut start = 0;
    cuts.into_iter()
        .chain(std::iter::once(samples))
        .map(|end| {
            let chunk = start..end;
            start = end;
            chunk
        })
        .filter(|chunk| !chunk.is_empty())
        .collect()
}

/// Buffers audio and its speech probabilities as they arrive and hands back
/// chunks once their cuts are certain, so they can be decoded while the user
/// is still speaking. Pushed all at once, the chunks are exactly
/// [`plan_chunks`]'s.
pub struct Chunker {
    frame_samples: usize,
    /// Audio from `start` on; everything before it is already in a chunk.
    pending: Vec<f32>,
    probs: Vec<f32>,
    /// False once audio arrives without probabilities: no more cuts.
    scored: bool,
    start: usize,
    len: usize,
}

impl Chunker {
    pub fn new(frame_samples: usize) -> Self {
        assert!(frame_samples > 0, "VAD frame size must be non-zero");
        Self {
            frame_samples,
            pending: Vec::new(),
            probs: Vec::new(),
            scored: true,
            start: 0,
            len: 0,
        }
    }

    /// Samples pushed so far, chunked or not. A caller checks this against
    /// the whole recording before trusting the chunks.
    pub fn samples(&self) -> usize {
        self.len
    }

    /// Adds audio; `probs` scores each whole frame of `pcm`, in order. Returns
    /// the chunks whose cuts this made certain, in order.
    pub fn push(&mut self, pcm: &[f32], probs: Option<&[f32]>) -> Vec<Vec<f32>> {
        if pcm.is_empty() {
            return Vec::new();
        }
        self.track_scores(pcm.len(), probs);
        self.pending.extend_from_slice(pcm);
        self.len += pcm.len();
        if !self.scored || self.len - self.start <= MAX_CHUNK_SAMPLES + LOOKAHEAD_SAMPLES {
            return Vec::new();
        }
        let segments = speech_segments(&self.probs, self.frame_samples, self.len);
        // A pause still open at the end moves its midpoint as it grows: wait
        // for speech.
        if segments.last().map(|segment| segment.end) != Some(self.len) {
            return Vec::new();
        }
        // One plan, committed while each cut's whole window plus the lookahead
        // is scored.
        let cuts = plan_cuts(&cut_candidates(&segments, self.len), self.len, self.start);
        let mut chunks = Vec::new();
        for cut in cuts {
            if self.len - self.start <= MAX_CHUNK_SAMPLES + LOOKAHEAD_SAMPLES {
                break;
            }
            chunks.push(self.take_until(cut));
        }
        chunks
    }

    /// Cuts what is left and returns the remaining chunks, in order.
    pub fn finish(mut self) -> Vec<Vec<f32>> {
        let mut chunks = Vec::new();
        if self.scored {
            let segments = speech_segments(&self.probs, self.frame_samples, self.len);
            let cuts = plan_cuts(&cut_candidates(&segments, self.len), self.len, self.start);
            for cut in cuts {
                chunks.push(self.take_until(cut));
            }
        } else if self.len - self.start > MAX_CHUNK_SAMPLES {
            info!(
                "No voice activity for the last {:.1}s; decoding it in one piece",
                seconds(self.len - self.start)
            );
        }
        if self.len > self.start {
            chunks.push(self.take_until(self.len));
        }
        chunks
    }

    fn track_scores(&mut self, samples: usize, probs: Option<&[f32]>) {
        if !self.scored {
            return;
        }
        let frame = self.frame_samples;
        match probs {
            Some(probs)
                if self.len.is_multiple_of(frame)
                    && (probs.len() == samples / frame
                        || probs.len() == samples.div_ceil(frame)) =>
            {
                self.probs.extend_from_slice(probs);
            }
            _ => {
                self.scored = false;
                debug!(
                    "Voice activity missing after {:.1}s; no more chunk cuts",
                    seconds(self.len)
                );
            }
        }
    }

    fn take_until(&mut self, cut: usize) -> Vec<f32> {
        debug!(
            "Long-form chunk: {:.1}-{:.1}s",
            seconds(self.start),
            seconds(cut)
        );
        let chunk = self.pending.drain(..cut - self.start).collect();
        self.start = cut;
        chunk
    }
}

/// Scripts written without spaces between words, and CJK punctuation.
static UNSPACED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"[\p{Han}\p{Hiragana}\p{Katakana}\p{Bopomofo}\p{Thai}\p{Lao}\p{Khmer}\p{Myanmar}\x{3000}-\x{303f}\x{ff00}-\x{ffef}]",
    )
    .unwrap()
});

fn is_unspaced(c: char) -> bool {
    UNSPACED.is_match(c.encode_utf8(&mut [0; 4]))
}

/// Chunk texts in order: a space between two chunks unless either side is
/// unspaced script.
pub fn join_transcripts<S: AsRef<str>>(texts: &[S]) -> String {
    let mut joined = String::new();
    let mut last: Option<char> = None;
    for text in texts {
        let text = text.as_ref().trim();
        let Some(first) = text.chars().next() else {
            continue;
        };
        if last.is_some_and(|last| !is_unspaced(last) && !is_unspaced(first)) {
            joined.push(' ');
        }
        joined.push_str(text);
        last = text.chars().last();
    }
    joined
}

fn seconds(samples: usize) -> f64 {
    samples as f64 / RATE as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: usize = 512;

    fn s(seconds: f64) -> usize {
        (seconds * RATE as f64).round() as usize
    }

    /// Frame probabilities: speech everywhere except the given pauses, in seconds.
    fn probs_with_pauses(seconds: f64, pauses: &[(f64, f64)]) -> Vec<f32> {
        (0..s(seconds) / FRAME)
            .map(|i| {
                let at = (i * FRAME) as f64 / RATE as f64;
                if pauses.iter().any(|&(start, end)| at >= start && at < end) {
                    0.0
                } else {
                    1.0
                }
            })
            .collect()
    }

    /// Cut positions, in tenths of a second.
    fn plan(seconds: f64, pauses: &[(f64, f64)]) -> Vec<f64> {
        let samples = s(seconds);
        let segments = speech_segments(&probs_with_pauses(seconds, pauses), FRAME, samples);
        plan_cuts(&cut_candidates(&segments, samples), samples, 0)
            .into_iter()
            .map(|cut| (cut as f64 / RATE as f64 * 10.0).round() / 10.0)
            .collect()
    }

    fn lengths(chunks: &[Vec<f32>]) -> Vec<usize> {
        chunks.iter().map(Vec::len).collect()
    }

    #[test]
    fn segments_follow_silero() {
        // Short blips dropped, short dips bridged, edges padded.
        let probs = [
            0., 0., 1., 1., 1., 1., 1., 1., 1., 1., 0., 0., 1., 1., 1., 1., 1., 1., 1., 1., 0., 0.,
            0., 0., 0., 1., 0., 0., 0., 0., 0.,
        ];
        let segments = speech_segments(&probs, FRAME, probs.len() * FRAME);
        // Frames 2-19 are one segment (a 2-frame dip is under 100 ms); the
        // lone frame 25 is under 250 ms.
        assert_eq!(segments, vec![2 * FRAME - 480..20 * FRAME + 480]);
    }

    #[test]
    fn segments_use_the_frame_size_given() {
        // The same speech scored in half-size frames gives the same segment.
        let probs: Vec<f32> = [0., 0., 1., 1., 1., 1., 1., 1., 1., 1., 0., 0., 0., 0., 0.]
            .iter()
            .flat_map(|&p| [p, p])
            .collect();
        let segments = speech_segments(&probs, FRAME / 2, 15 * FRAME);
        assert_eq!(segments, vec![2 * FRAME - 480..10 * FRAME + 480]);
    }

    #[test]
    fn cuts_prefer_long_pauses_near_30s() {
        assert!(
            plan(40.0, &[(20.0, 21.0)]).is_empty(),
            "40 s or less is never cut"
        );
        // A 1 s pause at 28-29 s beats a short one right at 30 s; 41.5 s
        // remain, so cut again.
        assert_eq!(
            plan(70.0, &[(28.0, 29.0), (30.0, 30.2), (55.0, 56.0)]),
            vec![28.5, 55.5]
        );
        // Without any pause, cuts are hard 40 s ones.
        assert_eq!(plan(100.0, &[]), vec![40.0, 80.0]);
        // A pause under 5 s in is never used.
        assert_eq!(plan(60.0, &[(2.0, 3.0)]), vec![40.0]);
    }

    #[test]
    fn cuts_avoid_leaving_a_short_last_chunk() {
        // The pause at 38 s would leave 4 s, so the weaker one at 10 s wins.
        assert_eq!(plan(42.0, &[(9.8, 10.2), (37.5, 38.5)]), vec![10.0]);
        // Without a pause, a short rest is split evenly instead of cut at 40 s.
        assert_eq!(plan(84.0, &[]), vec![40.0, 62.0]);
        // A pause that leaves a short rest still beats cutting mid-speech.
        assert_eq!(plan(40.5, &[(38.8, 39.2)]), vec![39.0]);
    }

    #[test]
    fn plan_chunks_covers_the_clip() {
        let probs = probs_with_pauses(100.0, &[(29.0, 30.0), (58.0, 59.0)]);
        let samples = probs.len() * FRAME;
        let chunks = plan_chunks(&probs, FRAME, samples);
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks[3].end, samples);
        assert!(chunks.windows(2).all(|pair| pair[0].end == pair[1].start));

        assert_eq!(
            plan_chunks(&[1.0; 10], FRAME, 10 * FRAME),
            vec![0..10 * FRAME]
        );
        assert!(plan_chunks(&[], FRAME, 0).is_empty());

        // Scores beyond the clip are ignored rather than cut past its end.
        let mut extra = probs.clone();
        extra.extend(probs_with_pauses(10.0, &[(4.0, 5.0)]));
        assert_eq!(plan_chunks(&extra, FRAME, samples), chunks);
    }

    #[test]
    fn joins_put_spaces_only_between_spaced_scripts() {
        assert_eq!(
            join_transcripts(&[" Hello there. ", "", "General Kenobi."]),
            "Hello there. General Kenobi."
        );
        assert_eq!(
            join_transcripts(&["今天天气很好。", "我们去公园吧。"]),
            "今天天气很好。我们去公园吧。"
        );
        assert_eq!(join_transcripts(&["I said", "你好"]), "I said你好");
        assert_eq!(
            join_transcripts(&["안녕하세요.", "반갑습니다."]),
            "안녕하세요. 반갑습니다."
        );
        // CJK Extension B, outside the Basic Multilingual Plane.
        assert_eq!(join_transcripts(&["你好𠀀", "我们"]), "你好𠀀我们");
    }

    /// 100 s of audio, with pauses at 29-30 s and 58-59 s.
    fn long_clip() -> (Vec<f32>, Vec<f32>) {
        let probs = probs_with_pauses(100.0, &[(29.0, 30.0), (58.0, 59.0)]);
        (vec![0.0; probs.len() * FRAME], probs)
    }

    #[test]
    fn live_chunks_arrive_before_the_end_at_the_same_cuts() {
        let (pcm, probs) = long_clip();
        let mut once = Chunker::new(FRAME);
        let mut offline = once.push(&pcm, Some(&probs));
        offline.extend(once.finish());
        let planned: Vec<usize> = plan_chunks(&probs, FRAME, pcm.len())
            .iter()
            .map(|chunk| chunk.len())
            .collect();
        assert_eq!(lengths(&offline), planned);

        let mut chunker = Chunker::new(FRAME);
        let mut live = Vec::new();
        for (frame, prob) in pcm.chunks(FRAME).zip(&probs) {
            live.extend(chunker.push(frame, Some(std::slice::from_ref(prob))));
        }
        // A cut is made whenever more than 45 s is waiting: 2 of the 4 chunks
        // before the end.
        assert_eq!(live.len(), 2, "chunks are ready while audio still arrives");
        live.extend(chunker.finish());
        assert_eq!(lengths(&live), planned);
    }

    #[test]
    fn live_cuts_wait_for_a_pause_to_end() {
        // At 45 s the pause so far is 34-45 s, a cut at 39.5 s; the whole
        // pause's midpoint, 42 s, is out of reach, so the plan cuts at 40 s.
        let probs = probs_with_pauses(90.0, &[(34.0, 50.0)]);
        let pcm = vec![0.0; probs.len() * FRAME];
        let planned: Vec<usize> = plan_chunks(&probs, FRAME, pcm.len())
            .iter()
            .map(|chunk| chunk.len())
            .collect();
        assert_eq!(planned, vec![s(40.0), s(40.0), pcm.len() - s(80.0)]);

        let mut chunker = Chunker::new(FRAME);
        let mut live = Vec::new();
        for (piece, piece_probs) in pcm.chunks(16 * FRAME).zip(probs.chunks(16)) {
            live.extend(chunker.push(piece, Some(piece_probs)));
        }
        live.extend(chunker.finish());
        assert_eq!(lengths(&live), planned);
    }

    #[test]
    fn audio_without_probabilities_is_one_piece_after_the_last_cut() {
        let (pcm, probs) = long_clip();
        let scored = s(50.0) - s(50.0) % FRAME;
        let mut chunker = Chunker::new(FRAME);
        let mut chunks = chunker.push(&pcm[..scored], Some(&probs[..scored / FRAME]));
        chunks.extend(chunker.push(&pcm[scored..], None));
        chunks.extend(chunker.finish());
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len() + chunks[1].len(), pcm.len());
        assert!(chunks[1].len() > s(40.0), "the unscored rest is one piece");
    }
}
