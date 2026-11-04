//! The encodings of text files: how the bytes of a file become the text of a document, and back.
//!
//! A file is read as UTF-8 when it is valid UTF-8, as the encoding of its byte order mark when it
//! has one (UTF-8, UTF-16), and as Windows-1252 otherwise, the most common of the old encodings.
//! The status bar shows the encoding, reopens the file with another one or converts it.

use encoding_rs::Encoding;

/// An encoding, and whether the file starts with a byte order mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextEncoding {
    pub encoding: &'static Encoding,
    pub bom: bool,
}

impl Default for TextEncoding {
    fn default() -> Self {
        UTF8
    }
}

pub const UTF8: TextEncoding = TextEncoding {
    encoding: encoding_rs::UTF_8,
    bom: false,
};

/// The encodings offered in the status bar.
pub const ALL: [TextEncoding; 11] = [
    UTF8,
    TextEncoding {
        encoding: encoding_rs::UTF_8,
        bom: true,
    },
    TextEncoding {
        encoding: encoding_rs::UTF_16LE,
        bom: true,
    },
    TextEncoding {
        encoding: encoding_rs::UTF_16BE,
        bom: true,
    },
    TextEncoding {
        encoding: encoding_rs::WINDOWS_1252,
        bom: false,
    },
    TextEncoding {
        encoding: encoding_rs::ISO_8859_15,
        bom: false,
    },
    TextEncoding {
        encoding: encoding_rs::WINDOWS_1251,
        bom: false,
    },
    TextEncoding {
        encoding: encoding_rs::SHIFT_JIS,
        bom: false,
    },
    TextEncoding {
        encoding: encoding_rs::GBK,
        bom: false,
    },
    TextEncoding {
        encoding: encoding_rs::BIG5,
        bom: false,
    },
    TextEncoding {
        encoding: encoding_rs::EUC_KR,
        bom: false,
    },
];

impl TextEncoding {
    /// The name shown in the status bar: `UTF-8`, `UTF-8 BOM`, `UTF-16 LE`, `windows-1252`…
    pub fn label(self) -> String {
        let name = self.encoding.name();
        match name {
            "UTF-16LE" => "UTF-16 LE".into(),
            "UTF-16BE" => "UTF-16 BE".into(),
            "UTF-8" if self.bom => "UTF-8 BOM".into(),
            _ => name.into(),
        }
    }

    fn is_utf16(self) -> bool {
        self.encoding == encoding_rs::UTF_16LE || self.encoding == encoding_rs::UTF_16BE
    }

    /// The bytes of `text` in this encoding, or the first character it cannot write.
    pub fn encode(self, text: &str) -> Result<Vec<u8>, char> {
        if self.is_utf16() {
            let big = self.encoding == encoding_rs::UTF_16BE;
            let mut out = Vec::with_capacity(text.len() * 2 + 2);
            for unit in std::iter::once(0xFEFF).chain(text.encode_utf16()) {
                out.extend_from_slice(&if big {
                    unit.to_be_bytes()
                } else {
                    unit.to_le_bytes()
                });
            }
            return Ok(out);
        }
        if self.encoding == encoding_rs::UTF_8 {
            let mut out = Vec::with_capacity(text.len() + 3);
            if self.bom {
                out.extend_from_slice(b"\xEF\xBB\xBF");
            }
            out.extend_from_slice(text.as_bytes());
            return Ok(out);
        }
        let (bytes, _, failed) = self.encoding.encode(text);
        if failed {
            // The first character that does not come back the same.
            let missing = text
                .chars()
                .find(|c| {
                    let mut buffer = [0u8; 4];
                    self.encoding.encode(c.encode_utf8(&mut buffer)).2
                })
                .unwrap_or('?');
            return Err(missing);
        }
        Ok(bytes.into_owned())
    }

    /// The text of `bytes` read in this encoding (its byte order mark left out).
    pub fn decode(self, bytes: &[u8]) -> String {
        let bytes = match Encoding::for_bom(bytes) {
            Some((found, length)) if found == self.encoding => &bytes[length..],
            _ => bytes,
        };
        self.encoding
            .decode_without_bom_handling(bytes)
            .0
            .into_owned()
    }
}

/// The encoding of the bytes of a file, and its text: by its byte order mark, else UTF-8 when
/// it is valid, else Windows-1252.
pub fn detect(bytes: &[u8]) -> (String, TextEncoding) {
    if let Some((encoding, length)) = Encoding::for_bom(bytes) {
        let text = encoding
            .decode_without_bom_handling(&bytes[length..])
            .0
            .into_owned();
        return (
            text,
            TextEncoding {
                encoding,
                bom: true,
            },
        );
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_string(), UTF8),
        Err(_) => {
            let encoding = TextEncoding {
                encoding: encoding_rs::WINDOWS_1252,
                bom: false,
            };
            (encoding.decode(bytes), encoding)
        }
    }
}

/// The ends of lines of a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
    Cr,
}

impl LineEnding {
    pub const ALL: [LineEnding; 3] = [LineEnding::Lf, LineEnding::CrLf, LineEnding::Cr];

    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::CrLf => "CRLF",
            Self::Cr => "CR",
        }
    }

    fn text(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }

    /// The ending of the first line that has one, LF without any.
    pub fn of(text: &str) -> Self {
        match text.find(['\r', '\n']) {
            Some(i) if text[i..].starts_with("\r\n") => Self::CrLf,
            Some(i) if text[i..].starts_with('\r') => Self::Cr,
            _ => Self::Lf,
        }
    }

    /// `text` with every end of line made this one.
    pub fn apply(self, text: &str) -> String {
        text.replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', self.text())
    }
}

#[cfg(test)]
#[path = "../test/encoding/tests.rs"]
mod tests;
