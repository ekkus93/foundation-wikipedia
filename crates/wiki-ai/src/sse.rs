//! Bounded incremental Server-Sent Events framing for opt-in LLM streams.
//!
//! This parser does not open a connection, follow redirects, parse model JSON,
//! or infer that a stream completed successfully. The provider adapter must
//! validate the application payload and its explicit terminal marker.

pub const MAX_SSE_LINE_BYTES: usize = 65_536;
pub const MAX_SSE_EVENT_BYTES: usize = 131_072;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SseError {
    LineTooLarge,
    EventTooLarge,
    InvalidUtf8,
    IncompleteFrame,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    pub event_type: Option<String>,
    pub data: String,
}

#[derive(Default)]
pub struct SseDecoder {
    pending_line: Vec<u8>,
    data: String,
    has_data: bool,
    event_type: Option<String>,
    saw_cr: bool,
    seen_first_line: bool,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process arbitrary byte chunks, including split CRLF and UTF-8 sequences.
    /// Reject oversized lines and events before accepting more untrusted bytes.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, SseError> {
        let mut events = Vec::new();
        for &byte in chunk {
            if self.saw_cr {
                self.saw_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }
            match byte {
                b'\r' => {
                    self.accept_line(&mut events)?;
                    self.saw_cr = true;
                }
                b'\n' => self.accept_line(&mut events)?,
                _ => {
                    if self.pending_line.len() >= MAX_SSE_LINE_BYTES {
                        return Err(SseError::LineTooLarge);
                    }
                    self.pending_line.push(byte);
                }
            }
        }
        Ok(events)
    }

    /// A provider must not treat a truncated final frame as a complete answer.
    pub fn finish(&self) -> Result<(), SseError> {
        if self.pending_line.is_empty() && !self.has_data && self.event_type.is_none() {
            Ok(())
        } else {
            Err(SseError::IncompleteFrame)
        }
    }

    fn accept_line(&mut self, events: &mut Vec<SseEvent>) -> Result<(), SseError> {
        let raw_line =
            std::str::from_utf8(&self.pending_line).map_err(|_| SseError::InvalidUtf8)?;
        // SSE permits exactly one UTF-8 BOM at the beginning of the stream.
        // Strip it from the first line only, even when network chunks split it.
        let line = if self.seen_first_line {
            raw_line
        } else {
            self.seen_first_line = true;
            raw_line.strip_prefix('\u{feff}').unwrap_or(raw_line)
        };
        if line.is_empty() {
            if self.has_data {
                events.push(SseEvent {
                    event_type: self.event_type.take(),
                    data: std::mem::take(&mut self.data),
                });
                self.has_data = false;
            }
            self.event_type = None;
        } else if !line.starts_with(':') {
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "data" => {
                    let additional = value.len() + usize::from(self.has_data);
                    if self.data.len().saturating_add(additional) > MAX_SSE_EVENT_BYTES {
                        return Err(SseError::EventTooLarge);
                    }
                    if self.has_data {
                        self.data.push('\n');
                    }
                    self.data.push_str(value);
                    self.has_data = true;
                }
                "event" => {
                    if value.len() > MAX_SSE_LINE_BYTES {
                        return Err(SseError::LineTooLarge);
                    }
                    self.event_type = Some(value.to_owned());
                }
                _ => {}
            }
        }
        self.pending_line.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_split_crlf_utf8_multiline_data_and_comments() {
        let mut decoder = SseDecoder::new();
        assert!(decoder
            .push(b": keepalive\r\nevent: message\r\ndata: {\"text\":\"")
            .unwrap()
            .is_empty());
        assert!(decoder.push(&[0xe2, 0x98]).unwrap().is_empty());
        assert!(decoder.push(&[0x83, b'"', b'}', b'\r']).unwrap().is_empty());
        let events = decoder.push(b"\ndata: second\r\n\r\n").unwrap();
        assert_eq!(
            events,
            vec![SseEvent {
                event_type: Some("message".into()),
                data: "{\"text\":\"☃\"}\nsecond".into(),
            }]
        );
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn handles_multiple_events_and_terminal_marker_without_assuming_success() {
        let mut decoder = SseDecoder::new();
        let events = decoder.push(b"data: one\n\ndata: [DONE]\n\n").unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "one");
        assert_eq!(events[1].data, "[DONE]");
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn preserves_empty_data_fields_and_multiline_boundaries() {
        let mut decoder = SseDecoder::new();
        let events = decoder
            .push(b"data:\n\ndata: first\ndata:\ndata: last\n\ndata:\ndata: tail\n\n")
            .unwrap();
        assert_eq!(
            events,
            vec![
                SseEvent {
                    event_type: None,
                    data: String::new(),
                },
                SseEvent {
                    event_type: None,
                    data: "first\n\nlast".into(),
                },
                SseEvent {
                    event_type: None,
                    data: "\ntail".into(),
                },
            ]
        );
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn empty_data_is_a_real_incomplete_event_until_delimited() {
        let mut decoder = SseDecoder::new();
        decoder.push(b"event: message\ndata: \n").unwrap();
        assert_eq!(decoder.finish(), Err(SseError::IncompleteFrame));
        assert_eq!(
            decoder.push(b"\n").unwrap(),
            vec![SseEvent {
                event_type: Some("message".into()),
                data: String::new(),
            }]
        );
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn event_without_data_does_not_dispatch() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push(b"event: ignored\n\n").unwrap().is_empty());
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn strips_only_the_initial_utf8_bom_even_across_network_chunks() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push(&[0xef]).unwrap().is_empty());
        assert!(decoder.push(&[0xbb]).unwrap().is_empty());
        assert!(decoder
            .push(b"\xbfdata: hello\n\n")
            .unwrap()
            .iter()
            .any(|event| { event.event_type.is_none() && event.data == "hello" }));
        assert_eq!(decoder.finish(), Ok(()));

        // A second BOM is not a stream prelude; it must not turn a
        // previously unrecognized field into a valid data field.
        let unexpected_bom = "\u{feff}data: ignored\n\n";
        assert!(decoder.push(unexpected_bom.as_bytes()).unwrap().is_empty());
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn rejects_truncated_or_invalid_frames() {
        let mut decoder = SseDecoder::new();
        decoder.push(b"data: incomplete\n").unwrap();
        assert_eq!(decoder.finish(), Err(SseError::IncompleteFrame));
        let mut decoder = SseDecoder::new();
        assert_eq!(decoder.push(b"data: \xff\n"), Err(SseError::InvalidUtf8));
    }

    #[test]
    fn enforces_line_and_accumulated_event_bounds() {
        let mut decoder = SseDecoder::new();
        assert_eq!(
            decoder.push(&vec![b'x'; MAX_SSE_LINE_BYTES + 1]),
            Err(SseError::LineTooLarge)
        );
        let mut decoder = SseDecoder::new();
        let line = format!("data: {}\n", "x".repeat(MAX_SSE_LINE_BYTES - 6));
        assert!(decoder.push(line.as_bytes()).unwrap().is_empty());
        assert!(decoder.push(line.as_bytes()).unwrap().is_empty());
        assert_eq!(decoder.push(line.as_bytes()), Err(SseError::EventTooLarge));
    }
}
