//! The minimal DOI normaliser (`specs/101` §11, slice E in its smallest form; used first by slice B, reused by slice D).
//!
//! One job: turn the several spellings of a DOI into one key so that two records of the same work meet. Strip the scheme and the
//! resolver prefixes, trim, lowercase (a DOI is case-insensitive), and check the shape `10.NNNN/suffix`.
//!
//! **It is used for matching only.** A normalised identifier is never a trust escalator (`specs/101` §11): nothing reads a DOI as a
//! reason to count, rank, trust or permit anything. Identifiers the data marks as missing (blank, `unavailable`) give `None`.

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
}
