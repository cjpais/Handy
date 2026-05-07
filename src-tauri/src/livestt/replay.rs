#[derive(Debug, Default, Clone)]
pub struct LiveSttReplayBuffer {
    chunks: Vec<Vec<u8>>,
    #[cfg(test)]
    total_bytes: usize,
}

impl LiveSttReplayBuffer {
    pub fn append_pcm_chunk(&mut self, bytes: Vec<u8>) {
        self.track_total_bytes(bytes.len());
        self.chunks.push(bytes);
    }

    #[cfg(test)]
    fn track_total_bytes(&mut self, len: usize) {
        self.total_bytes += len;
    }

    #[cfg(not(test))]
    fn track_total_bytes(&mut self, _len: usize) {}

    #[cfg(test)]
    fn iter_chunks(&self) -> impl Iterator<Item = &[u8]> {
        self.chunks.iter().map(Vec::as_slice)
    }

    pub fn snapshot_chunks(&self) -> Vec<Vec<u8>> {
        self.chunks.clone()
    }

    #[cfg(test)]
    fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    #[cfg(test)]
    fn clear(&mut self) {
        self.chunks.clear();
        self.total_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_buffer_empty_snapshot_is_empty() {
        let buffer = LiveSttReplayBuffer::default();

        assert!(buffer.snapshot_chunks().is_empty());
    }

    #[test]
    fn replay_buffer_appends_chunks_in_order() {
        let mut buffer = LiveSttReplayBuffer::default();

        buffer.append_pcm_chunk(vec![1, 2]);
        buffer.append_pcm_chunk(vec![3]);
        buffer.append_pcm_chunk(vec![4, 5, 6]);

        let chunks: Vec<Vec<u8>> = buffer.iter_chunks().map(|chunk| chunk.to_vec()).collect();
        assert_eq!(chunks, vec![vec![1, 2], vec![3], vec![4, 5, 6]]);
    }

    #[test]
    fn replay_buffer_preserves_all_chunks_in_snapshot() {
        let mut buffer = LiveSttReplayBuffer::default();

        buffer.append_pcm_chunk(vec![10]);
        buffer.append_pcm_chunk(vec![20, 30]);

        assert_eq!(buffer.snapshot_chunks(), vec![vec![10], vec![20, 30]]);
    }

    #[test]
    fn replay_buffer_snapshot_returns_three_chunks_in_order() {
        let mut buffer = LiveSttReplayBuffer::default();

        buffer.append_pcm_chunk(vec![1]);
        buffer.append_pcm_chunk(vec![2, 3]);
        buffer.append_pcm_chunk(vec![4, 5, 6]);

        assert_eq!(
            buffer.snapshot_chunks(),
            vec![vec![1], vec![2, 3], vec![4, 5, 6]]
        );
    }

    #[test]
    fn replay_buffer_snapshot_does_not_clear_buffer() {
        let mut buffer = LiveSttReplayBuffer::default();

        buffer.append_pcm_chunk(vec![1, 2]);
        buffer.append_pcm_chunk(vec![3, 4]);

        assert_eq!(buffer.snapshot_chunks(), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(buffer.snapshot_chunks(), vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(buffer.total_bytes(), 4);
    }

    #[test]
    fn replay_buffer_snapshot_chunks_are_cloned() {
        let mut buffer = LiveSttReplayBuffer::default();

        buffer.append_pcm_chunk(vec![1, 2]);
        let mut snapshot = buffer.snapshot_chunks();
        snapshot[0][0] = 9;

        assert_eq!(buffer.snapshot_chunks(), vec![vec![1, 2]]);
    }

    #[test]
    fn replay_buffer_tracks_total_bytes() {
        let mut buffer = LiveSttReplayBuffer::default();

        buffer.append_pcm_chunk(vec![1, 2, 3]);
        buffer.append_pcm_chunk(vec![4]);

        assert_eq!(buffer.total_bytes(), 4);
    }

    #[test]
    fn replay_buffer_clear_removes_chunks_and_total() {
        let mut buffer = LiveSttReplayBuffer::default();

        buffer.append_pcm_chunk(vec![1, 2, 3]);
        buffer.clear();

        assert_eq!(buffer.total_bytes(), 0);
        assert_eq!(buffer.iter_chunks().count(), 0);
    }
}
