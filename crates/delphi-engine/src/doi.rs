//! The identifier normaliser (`specs/101` §11, slice E; the DOI form was used first by slice B and reused by slice D).
//!
//! One job: turn the several spellings of an identifier into one key so that two records of the same thing meet. For a DOI: strip the
//! scheme and the resolver prefixes, trim, lowercase (a DOI is case-insensitive), and check the shape `10.NNNN/suffix`. For an ORCID iD
//! ([`orcid`]) and a ROR id ([`ror`]) the shape and the check digits are verified as well.
//!
//! **It is used for matching only.** A normalised identifier is never a trust escalator (`specs/101` §11): nothing reads one as a
//! reason to count, rank, trust or permit anything; the one place a DOI reaches the origin count is `corroboration::support`, which
//! merges two citations of the same work into one origin and so can only lower the count. Identifiers the data marks as missing
//! (blank, `unavailable`) give `None`.
//!
//! What was verified (2026-11-11/12, `specs/101` slice E). ROR (<https://ror.readme.io/docs/identifier>): the id is 9 characters, the first
//! `0`, then six characters of Crockford base32 (lowercase, no `i l o u`), then two digits that are a checksum; the page gives the
//! pattern `^0[a-hj-km-np-tv-z0-9]{6}[0-9]{2}$` and the example `02mhbdp94`. The page does not say what the checksum covers; the code it
//! links to (`ror-community/ror-api`, `generaterorid.py`) computes it from the *integer* the six characters encode: `98 - ((n * 100) % 97)`,
//! zero-padded to two digits (by hand, `2mhbdp` is 88,649,142 and gives `94`, the page's example). ORCID (<https://info.orcid.org>, the
//! FAQ "Why do some ORCID iDs end with an X?"): the last character is a checksum from 0 to 10, 10 written `X`, and the example
//! `https://orcid.org/0000-0002-1694-233X` is given. The ORCID support page that states the algorithm (ISO/IEC 7064:2003, MOD 11-2, over the first
//! 15 digits, and that the `X` is a capital) answered 403 to a fetch, so those two points are NOT VALIDATED against the page itself; the
//! code below is checked against two real iDs (the FAQ's, and `0000-0002-1825-0097`, the support page's example as a search summary
//! reports it).

/// The resolver prefixes a DOI is written with, longest first.
const PREFIXES: [&str; 8] = ["https://dx.doi.org/", "http://dx.doi.org/", "https://doi.org/", "http://doi.org/", "dx.doi.org/", "doi.org/", "doi:", "doi "];

/// The longest DOI kept. Real DOIs are far shorter; a longer string is not one.
pub const MAX_LEN: usize = 200;

/// The normalised DOI, or `None` when `raw` is not shaped like one.
pub fn normalize(raw: &str) -> Option<String> {
    let mut s = raw.trim().to_string();
    if s.is_empty() || s.eq_ignore_ascii_case("unavailable") {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    for p in PREFIXES {
        if lower.starts_with(p) {
            s = s[p.len()..].to_string();
            break;
        }
    }
    let s = s.trim().trim_end_matches(['.', ',', ';', ')', ']', '>']).to_ascii_lowercase();
    shaped(&s).then_some(s)
}

/// `10.` + 4 to 9 digits + `/` + a non-empty suffix with no whitespace or control character.
fn shaped(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_LEN {
        return false;
    }
    let Some(rest) = s.strip_prefix("10.") else { return false };
    let Some((registrant, suffix)) = rest.split_once('/') else { return false };
    (4..=9).contains(&registrant.len()) && registrant.bytes().all(|b| b.is_ascii_digit()) && !suffix.is_empty() && !suffix.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The DOI a `doi.org` address names: `https://doi.org/10.1000/xyz` gives `10.1000/xyz`. Any other address gives `None`; the page's own
/// text is never searched for one.
pub fn from_url(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("https://").or_else(|| url.trim().strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    if !matches!(host.to_ascii_lowercase().as_str(), "doi.org" | "dx.doi.org" | "www.doi.org") {
        return None;
    }
    // a query string or fragment is not part of the identifier
    normalize(path.split(['?', '#']).next().unwrap_or(""))
}

/// Strip the first of `prefixes` that `raw` starts with (compared without regard to case).
fn strip_any<'a>(raw: &'a str, prefixes: &[&str]) -> &'a str {
    let lower = raw.to_ascii_lowercase();
    for p in prefixes {
        if lower.starts_with(p) {
            return &raw[p.len()..];
        }
    }
    raw
}

/// The check character of an ORCID iD for its first 15 digits (ISO/IEC 7064 MOD 11-2, as ORCID describes it): the running total is doubled
/// after each digit is added, and `(12 - total mod 11) mod 11` is the value, 10 written `X`.
fn orcid_check(first15: &str) -> Option<char> {
    let mut total: u32 = 0;
    for c in first15.chars() {
        total = (total + c.to_digit(10)?) * 2;
    }
    let value = (12 - total % 11) % 11;
    Some(if value == 10 { 'X' } else { char::from_digit(value, 10)? })
}

/// An ORCID iD in its one canonical spelling, `0000-0002-1694-233X`, or `None` when `raw` is not one: sixteen characters (fifteen digits and
/// a check character, a digit or `X`), written bare or with the four hyphenated groups, optionally behind `https://orcid.org/` or
/// `orcid:`, and the check character right. A lowercase `x` is accepted and written `X`, which makes matching more lenient, never less.
pub fn orcid(raw: &str) -> Option<String> {
    let s = strip_any(raw.trim(), &["https://orcid.org/", "http://orcid.org/", "orcid.org/", "orcid:"]).trim();
    if s.contains('-') {
        let groups: Vec<&str> = s.split('-').collect();
        if groups.len() != 4 || groups.iter().any(|g| g.len() != 4) {
            return None;
        }
    }
    let flat: String = s.chars().filter(|c| *c != '-').collect::<String>().to_ascii_uppercase();
    if flat.len() != 16 || !flat.is_ascii() {
        return None;
    }
    let (body, last) = flat.split_at(15);
    if !body.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    (orcid_check(body)? == last.chars().next()?).then(|| format!("{}-{}-{}-{}", &flat[0..4], &flat[4..8], &flat[8..12], &flat[12..16]))
}

/// Crockford base32 as ROR writes it: lowercase, without `i l o u`.
const CROCKFORD: &str = "0123456789abcdefghjkmnpqrstvwxyz";

/// A ROR id in its one canonical spelling, the nine bare characters (`02mhbdp94`), or `None` when `raw` is not one: a `0`, six Crockford
/// base32 characters and two digits, optionally behind `https://ror.org/` or `ror:`, and the two digits right. They are
/// `98 - ((n * 100) % 97)` for the integer `n` the six characters encode (from the generator code the ROR documentation links to).
pub fn ror(raw: &str) -> Option<String> {
    let s = strip_any(raw.trim(), &["https://ror.org/", "http://ror.org/", "ror.org/", "ror:"]).trim().to_ascii_lowercase();
    if s.len() != 9 || !s.is_ascii() || !s.starts_with('0') {
        return None;
    }
    let (body, check) = (&s[1..7], &s[7..9]);
    if !check.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut n: u64 = 0;
    for c in body.chars() {
        n = n * 32 + CROCKFORD.find(c)? as u64;
    }
    (format!("{:02}", 98 - (n * 100) % 97) == check).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spellings_of_one_doi_meet() {
        for s in ["10.1000/XYZ123", "doi:10.1000/xyz123", "DOI:10.1000/xyz123", "https://doi.org/10.1000/xyz123", "http://dx.doi.org/10.1000/XYZ123", "  doi.org/10.1000/xyz123.  ", "10.1000/xyz123)"] {
            assert_eq!(normalize(s).as_deref(), Some("10.1000/xyz123"), "{s:?}");
        }
    }

    #[test]
    fn what_is_not_a_doi_gives_none() {
        for s in ["", "   ", "unavailable", "Unavailable", "10.1000", "10.1000/", "10.12/abc", "10.1234567890/abc", "11.1000/abc", "10.1000/a b", "10.10a0/abc", "not a doi", "https://example.com/10.1000/abc"] {
            assert_eq!(normalize(s), None, "{s:?}");
        }
        assert_eq!(normalize(&format!("10.1000/{}", "a".repeat(MAX_LEN))), None, "too long");
    }

    #[test]
    fn only_a_doi_resolver_address_names_one() {
        assert_eq!(from_url("https://doi.org/10.1000/AbC?utm=1#x").as_deref(), Some("10.1000/abc"));
        assert_eq!(from_url("http://dx.doi.org/10.1000/abc").as_deref(), Some("10.1000/abc"));
        assert_eq!(from_url("https://example.com/doi.org/10.1000/abc"), None);
        assert_eq!(from_url("https://notdoi.org/10.1000/abc"), None);
        assert_eq!(from_url("https://doi.org/"), None);
        assert_eq!(from_url("ftp://doi.org/10.1000/abc"), None);
    }

    #[test]
    fn the_spellings_of_one_orcid_meet_and_the_check_character_is_checked() {
        // 0000-0002-1694-233X is the example on ORCID's FAQ about the X; 0000-0002-1825-0097 is the support page's.
        for s in ["0000-0002-1694-233X", "https://orcid.org/0000-0002-1694-233X", "http://orcid.org/0000-0002-1694-233x", "orcid:000000021694233X", "000000021694233X", " ORCID.org/0000-0002-1694-233X "] {
            assert_eq!(orcid(s).as_deref(), Some("0000-0002-1694-233X"), "{s:?}");
        }
        for s in ["0000-0002-1825-0097", "https://orcid.org/0000000218250097"] {
            assert_eq!(orcid(s).as_deref(), Some("0000-0002-1825-0097"), "{s:?}");
        }
        for s in ["", "unavailable", "0000-0002-1694-2330", "0000-0002-1825-0098", "0000-0002-1694-233", "0000-0002-1694-233XX", "0000-0002-16942-33X", "0000-0002-1694", "0000-0002-1694-23A3", "https://example.com/0000-0002-1694-233X"] {
            assert_eq!(orcid(s), None, "{s:?}");
        }
    }

    #[test]
    fn the_spellings_of_one_ror_id_meet_and_its_checksum_is_checked() {
        // 02mhbdp94 is the example on ROR's identifier page; its six characters are 88,649,142 and give check digits 94.
        for s in ["02mhbdp94", "https://ror.org/02mhbdp94", "http://ror.org/02MHBDP94", "ror.org/02mhbdp94", "ror:02mhbdp94", "  02mhbdp94 "] {
            assert_eq!(ror(s).as_deref(), Some("02mhbdp94"), "{s:?}");
        }
        for s in ["", "02mhbdp95", "12mhbdp94", "02mhbdp9", "02mhbdp944", "02mhbdi94", "02mhbdu94", "02mhbdp9x", "https://example.org/02mhbdp94", "0000000"] {
            assert_eq!(ror(s), None, "{s:?}");
        }
    }
}
