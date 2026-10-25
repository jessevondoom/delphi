//! The R4 readers (OpenDocument, EPUB, HTML, data files, mail, notebooks, RTF, images) through the public
//! `extract`, one fixture per feature (`specs/014` §9).

use crate::zipfile::testutil::make_zip;
use crate::*;

fn ex(bytes: &[u8], name: &str, req: Request) -> Result<Extract, DocError> {
    extract(bytes, name, &req, &Limits::default())
}

fn odf(mime: &str, content: &str) -> Vec<u8> {
    make_zip(&[("mimetype", mime.as_bytes(), 0), ("content.xml", content.as_bytes(), 8), ("META-INF/manifest.xml", b"<manifest/>", 0)])
}

#[test]
fn an_odt_reads_like_a_docx() {
    let c = r#"<office:document-content><office:automatic-styles><style:style><text:p>STYLE NOISE</text:p></style:style></office:automatic-styles><office:body><office:text>
<text:h text:outline-level="1">Plan</text:h>
<text:p>Hello <text:span>bold</text:span><text:s text:c="2"/>there <text:a xlink:href="https://e.com">link</text:a>.<text:note><text:note-citation>1</text:note-citation><text:note-body><text:p>A footnote.</text:p></text:note-body></text:note></text:p>
<text:list><text:list-item><text:p>one</text:p><text:list><text:list-item><text:p>nested</text:p></text:list-item></text:list></text:list-item></text:list>
<table:table><table:table-row><table:table-cell><text:p>A</text:p></table:table-cell><table:table-cell><text:p>B</text:p></table:table-cell></table:table-row><table:table-row><table:table-cell><text:p>1</text:p></table:table-cell><table:table-cell><text:p>2</text:p></table:table-cell></table:table-row></table:table>
<text:tracked-changes><text:changed-region><text:deletion><text:p>DELETED TEXT</text:p></text:deletion></text:changed-region></text:tracked-changes>
<text:p>after<draw:frame><draw:image/></draw:frame></text:p>
</office:text></office:body></office:document-content>"#;
    let e = ex(&odf("application/vnd.oasis.opendocument.text", c), "a.odt", Request::default()).unwrap();
    assert_eq!(e.format, Format::Odt);
    let t = &e.text;
    assert!(t.starts_with("# Plan\n\nHello bold  there [link](https://e.com).[^1]\n\n"), "{}", t);
    assert!(t.contains("- one\n  - nested\n\n| A | B |\n|---|---|\n| 1 | 2 |"), "{}", t);
    assert!(t.contains("## Notes\n\n[^1]: A footnote.") && !t.contains("DELETED") && !t.contains("STYLE NOISE"), "{}", t);
    assert!(e.loss.iter().any(|l| l.contains("1 image(s)")), "{:?}", e.loss);
}

fn ods_fixture() -> Vec<u8> {
    let c = r#"<office:document-content><office:body><office:spreadsheet>
<table:table table:name="Budget">
<table:table-row><table:table-cell office:value-type="string"><text:p>Item</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>Cost</text:p></table:table-cell></table:table-row>
<table:table-row><table:table-cell office:value-type="string"><text:p>Rent</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="1200"><text:p>$1,200.00</text:p></table:table-cell></table:table-row>
<table:table-row><table:table-cell office:value-type="string"><text:p>Total</text:p></table:table-cell><table:table-cell table:formula="of:=SUM([.B2:.B2])" office:value-type="float" office:value="1200"><text:p>$1,200.00</text:p></table:table-cell></table:table-row>
<table:table-row table:number-rows-repeated="1000000"><table:table-cell table:number-columns-repeated="16384"/></table:table-row>
</table:table>
<table:table table:name="Dates"><table:table-row><table:table-cell office:value-type="date" office:date-value="2026-10-25"/><table:table-cell table:number-columns-repeated="2" office:value-type="string"><text:p>x</text:p></table:table-cell></table:table-row></table:table>
</office:spreadsheet></office:body></office:document-content>"#;
    odf("application/vnd.oasis.opendocument.spreadsheet", c)
}

#[test]
fn an_ods_reads_like_an_xlsx_and_survives_huge_empty_repeats() {
    let e = ex(&ods_fixture(), "b.ods", Request::default()).unwrap();
    assert_eq!((e.format, e.unit, e.total_units), (Format::Ods, Unit::Sheet, Some(2)));
    let t = &e.text;
    assert!(t.contains("Spreadsheet with 2 sheets: 1: Budget, 2: Dates"), "{}", t);
    assert!(t.contains("## Sheet 1: Budget (3 rows × 2 columns · last cell B3)"), "{}", t);
    assert!(t.contains("| 2 | Rent | $1,200.00 |") && t.contains("| 3 | Total | $1,200.00 (=SUM([.B2:.B2])) |"), "{}", t);
    assert!(t.contains("| 1 | 2026-10-25 | x | x |"), "a date cell without display text shows its value: {}", t);
    let s = ex(&ods_fixture(), "b.ods", Request { sheet: Some("budget".into()), range: Some("A3:B3".into()), ..Request::default() }).unwrap();
    assert!(s.text.contains("Header (row 1): A=Item, B=Cost") && s.text.contains("| 3 | Total |") && !s.text.contains("| 2 |"), "{}", s.text);
    assert!(ex(&ods_fixture(), "b.ods", Request { sheet: Some("nope".into()), ..Request::default() }).unwrap_err().0.contains("1: Budget, 2: Dates"));
}

#[test]
fn an_odp_reads_slides_titles_lists_and_notes() {
    let c = r#"<office:document-content><office:body><office:presentation>
<draw:page draw:name="page1"><draw:frame presentation:class="title"><draw:text-box><text:p>Kickoff</text:p></draw:text-box></draw:frame>
<draw:frame presentation:class="outline"><draw:text-box><text:list><text:list-item><text:p>Goals</text:p></text:list-item></text:list></draw:text-box></draw:frame>
<draw:frame><draw:image/></draw:frame>
<presentation:notes><draw:frame><draw:text-box><text:p>Say hello</text:p></draw:text-box></draw:frame></presentation:notes></draw:page>
<draw:page draw:name="page2"><draw:frame presentation:class="title"><draw:text-box><text:p>Budget</text:p></draw:text-box></draw:frame></draw:page>
</office:presentation></office:body></office:document-content>"#;
    let z = odf("application/vnd.oasis.opendocument.presentation", c);
    let e = ex(&z, "d.odp", Request::default()).unwrap();
    assert_eq!((e.format, e.unit, e.total_units), (Format::Odp, Unit::Slide, Some(2)));
    assert!(e.text.contains("## Slide 1: Kickoff\n\n- Goals\n\nNotes: Say hello\n\n## Slide 2: Budget"), "{}", e.text);
    let p = ex(&z, "d.odp", Request { start: Some(2), end: Some(2), ..Request::default() }).unwrap();
    assert!(p.text.contains("## Slide 2") && !p.text.contains("Kickoff"), "{}", p.text);
}

fn epub() -> Vec<u8> {
    let opf = r#"<package><metadata><dc:title>My Book</dc:title><dc:creator>Ann Author</dc:creator></metadata><manifest><item id="c1" href="ch%201.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="ch2.xhtml" media-type="application/xhtml+xml"/><item id="css" href="s.css" media-type="text/css"/></manifest><spine><itemref idref="c1"/><itemref idref="css"/><itemref idref="c2"/></spine></package>"#;
    make_zip(&[
        ("mimetype", b"application/epub+zip", 0),
        ("META-INF/container.xml", br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#, 0),
        ("OEBPS/content.opf", opf.as_bytes(), 8),
        ("OEBPS/ch 1.xhtml", b"<html><body><h1>One</h1><p>First chapter &amp; more.</p></body></html>", 8),
        ("OEBPS/ch2.xhtml", b"<html><body><h1>Two</h1><p>Second chapter.</p><script>x()</script></body></html>", 8),
    ])
}

#[test]
fn an_epub_reads_its_spine_in_order_and_pages_by_chapter() {
    let e = ex(&epub(), "b.epub", Request::default()).unwrap();
    assert_eq!(e.format, Format::Epub);
    assert!(e.text.starts_with("EPUB · 2 chapters · My Book · Ann Author\n\n## Chapter 1\n\n# One\n\nFirst chapter & more.\n\n## Chapter 2\n\n# Two\n\nSecond chapter."), "{}", e.text);
    assert!(e.loss.iter().any(|l| l.contains("script")), "{:?}", e.loss);
    let c2 = ex(&epub(), "b.epub", Request { start: Some(2), ..Request::default() }).unwrap();
    assert!(c2.text.contains("## Chapter 2") && !c2.text.contains("First chapter"), "{}", c2.text);
    assert!(ex(&epub(), "b.epub", Request { start: Some(5), ..Request::default() }).unwrap_err().0.contains("2 chapters"));
}

#[test]
fn html_and_data_files_are_converted_and_drilled_into() {
    let e = ex(b"<title>T</title><h1>Head</h1><p>Body <b>bold</b></p><script>bad()</script>", "p.html", Request::default()).unwrap();
    assert!(e.text.contains("Title: T\n\n# Head\n\nBody **bold**") && e.loss.iter().any(|l| l.contains("1 script")), "{}", e.text);
    let j = ex(br#"{"a":[1,2,{"b":"deep"}]}"#, "x.json", Request { member: Some("a.2.b".into()), ..Request::default() }).unwrap();
    assert!(j.text.ends_with("\"deep\""), "{}", j.text);
    let y = ex(b"k:\n  - 1\n  - v: ok\n", "x.yaml", Request { member: Some("k.1.v".into()), ..Request::default() }).unwrap();
    assert!(y.text.ends_with("\"ok\""), "{}", y.text);
    assert!(ex(b"plain text", "x.txt", Request { member: Some("a".into()), ..Request::default() }).unwrap_err().0.contains("no members"));
}

#[test]
fn mail_calendar_notebook_and_rtf_dispatch() {
    let m = ex(b"Subject: Hi\nFrom: a@x.com\n\nBody here\n", "m.eml", Request::default()).unwrap();
    assert!(m.text.contains("Subject: Hi") && m.text.contains("Body here"), "{}", m.text);
    let mb = b"From a@x.com Sat Oct 24 10:00:00 2026\nSubject: One\n\nfirst\n\nFrom b@x.com Sat Oct 24 11:00:00 2026\nSubject: Two\n\nsecond\n";
    let l = ex(mb, "inbox", Request::default()).unwrap();
    assert_eq!((l.format, l.total_units), (Format::Mbox, Some(2)));
    assert!(ex(mb, "inbox", Request { member: Some("#1".into()), ..Request::default() }).unwrap().text.contains("second"));
    let c = ex(b"BEGIN:VEVENT\nSUMMARY:Standup\nDTSTART:20261026T090000Z\nEND:VEVENT\n", "c.ics", Request::default()).unwrap();
    assert!(c.text.contains("## Standup\nWhen: 2026-10-26 09:00 UTC"), "{}", c.text);
    let n = ex(br#"{"cells":[{"cell_type":"markdown","source":"hi"}]}"#, "n.ipynb", Request::default()).unwrap();
    assert!(n.text.contains("Notebook · 1 cell") && n.text.contains("hi"), "{}", n.text);
    let r = ex(br"{\rtf1\ansi Hello\par World}", "d.rtf", Request::default()).unwrap();
    assert_eq!(r.text, "Hello\nWorld");
}

#[test]
fn images_report_metadata_and_never_pixels() {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&[0, 0, 0, 16, 0, 0, 0, 9, 8, 2, 0, 0, 0]);
    let e = ex(&png, "i.png", Request::default()).unwrap();
    assert!(e.supported && e.text.contains("PNG image") && e.text.contains("16×9 pixels · 8-bit RGB"), "{}", e.text);
    assert!(e.loss.iter().any(|l| l.contains("pixel content is not read")));
}

#[test]
fn the_new_readers_never_panic_on_flipped_bits() {
    let mut x: u64 = 0xbead_ed00_dead_beef;
    for z in [epub(), ods_fixture(), odf("application/vnd.oasis.opendocument.text", "<office:text><text:p>x</text:p></office:text>")] {
        for _ in 0..300 {
            let mut f = z.clone();
            for _ in 0..4 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let at = (x as usize) % f.len();
                f[at] ^= (x >> 40) as u8 | 1;
            }
            let _ = extract(&f, "x.zip", &Request::default(), &Limits::default());
        }
    }
}
