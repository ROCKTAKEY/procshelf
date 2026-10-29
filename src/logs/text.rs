#[derive(Default)]
pub(crate) struct OutputDecoder {
    pending: Vec<u8>,
}
impl OutputDecoder {
    pub(crate) fn decode(&mut self, bytes: &[u8], eof: bool) -> String {
        let mut input = std::mem::take(&mut self.pending);
        input.extend_from_slice(bytes);
        let mut remaining = input.as_slice();
        let mut text = String::new();
        while !remaining.is_empty() {
            match std::str::from_utf8(remaining) {
                Ok(valid) => {
                    text.push_str(valid);
                    break;
                }
                Err(error) => {
                    let (valid, tail) = remaining.split_at(error.valid_up_to());
                    text.push_str(
                        std::str::from_utf8(valid)
                            .expect("UTF-8 validator returned a valid prefix"),
                    );
                    match error.error_len() {
                        Some(length) => {
                            text.push('\u{fffd}');
                            remaining = &tail[length..];
                        }
                        None => {
                            if eof {
                                text.push('\u{fffd}');
                            } else {
                                self.pending.extend_from_slice(tail);
                            }
                            break;
                        }
                    }
                }
            }
        }
        text
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_utf8_characters_split_across_pipe_reads() {
        let mut decoder = OutputDecoder::default();
        assert_eq!(decoder.decode(&[0xe6, 0x97], false), "");
        assert_eq!(decoder.decode(&[0xa5, 0xff, 0xe6], false), "日�");
        assert_eq!(decoder.decode(&[], true), "�");
    }
}
