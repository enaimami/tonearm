//! Small encodings the core writes itself instead of taking a crate for them.
//!
//! Base64 is needed in three places: the AcoustID fingerprint (the URL-safe
//! alphabet without padding), the bytes a plugin fetches through
//! `host.http` in binary mode, and the `data:` URIs covers reach the
//! interface as (D-076) — the webview's CSP loads no remote image. One
//! implementation serves all three: a second copy would drift (D-051).
//! A crate would add to a tree that goes to mobile, for forty lines.

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode(data: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map_or(0, |b| u32::from(*b));
        let b2 = chunk.get(2).map_or(0, |b| u32::from(*b));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        // As many 6-bit digits are written as the input has bytes: 1 byte → 2
        // digits, 2 bytes → 3 digits, 3 bytes → 4 digits.
        let digits = chunk.len() + 1;
        for i in 0..digits {
            let shift = 18 - 6 * i;
            let index = ((triple >> shift) & 0x3f) as usize;
            out.push(char::from(alphabet[index]));
        }
        if pad {
            for _ in digits..4 {
                out.push('=');
            }
        }
    }
    out
}

/// The standard alphabet with `=` padding (RFC 4648 §4) — what `data:` URIs
/// and `atob` read.
#[must_use]
pub(crate) fn base64_standard(data: &[u8]) -> String {
    encode(data, STANDARD, true)
}

/// The URL-safe alphabet without padding (RFC 4648 §5) — what AcoustID wants
/// in a query string, where `+`, `/` and `=` would need escaping.
#[must_use]
#[cfg_attr(not(feature = "fingerprint"), allow(dead_code))]
pub(crate) fn base64_url_nopad(data: &[u8]) -> String {
    encode(data, URL_SAFE, false)
}

/// Decodes either alphabet, with or without padding. ASCII whitespace is
/// skipped — a plugin that wraps its output at 76 columns is not wrong.
///
/// `None` for anything else: a character outside both alphabets, padding in
/// the middle, or a length no encoder produces. A broken payload is not
/// half-decoded into a broken image.
#[must_use]
pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    let mut padding = 0usize;
    let mut digits = 0usize;
    for byte in text.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return None;
        }
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        digits += 1;
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buffer >> bits) & 0xff).ok()?);
        }
    }
    // A single digit left over carries no whole byte: no encoder writes that.
    if digits % 4 == 1 || padding > 2 {
        return None;
    }
    Some(out)
}

/// `data:<mime>;base64,<…>` — the only way an image reaches the webview.
#[must_use]
pub(crate) fn data_uri(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64_standard(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4648 §10 vectors, in both alphabets.
    #[test]
    fn both_alphabets_match_the_rfc_vectors() {
        for (raw, padded, bare) in [
            ("", "", ""),
            ("f", "Zg==", "Zg"),
            ("fo", "Zm8=", "Zm8"),
            ("foo", "Zm9v", "Zm9v"),
            ("foob", "Zm9vYg==", "Zm9vYg"),
            ("fooba", "Zm9vYmE=", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_standard(raw.as_bytes()), padded);
            assert_eq!(base64_url_nopad(raw.as_bytes()), bare);
            assert_eq!(base64_decode(padded).as_deref(), Some(raw.as_bytes()));
            assert_eq!(base64_decode(bare).as_deref(), Some(raw.as_bytes()));
        }
        // The two alphabets differ only in the last two digits.
        assert_eq!(base64_standard(&[0xfb, 0xff]), "+/8=");
        assert_eq!(base64_url_nopad(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn decoding_round_trips_every_byte_value() {
        let all: Vec<u8> = (0..=255u8).collect();
        assert_eq!(base64_decode(&base64_standard(&all)), Some(all.clone()));
        assert_eq!(base64_decode(&base64_url_nopad(&all)), Some(all));
    }

    #[test]
    fn a_broken_payload_is_refused_not_half_decoded() {
        assert_eq!(
            base64_decode("Zm9v!"),
            None,
            "a character outside both alphabets"
        );
        assert_eq!(base64_decode("Zg==Zg"), None, "padding in the middle");
        assert_eq!(base64_decode("Zm9vY"), None, "a length no encoder writes");
        assert_eq!(
            base64_decode("Zm9v\nYmFy\n").as_deref(),
            Some(&b"foobar"[..]),
            "line breaks are whitespace, not an error"
        );
    }

    #[test]
    fn a_data_uri_names_its_type() {
        assert_eq!(data_uri("image/png", b"foo"), "data:image/png;base64,Zm9v");
    }
}
