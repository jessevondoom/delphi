//! Office readers against small hand-made packages: one fixture per feature (`specs/014` §9).

use crate::zipfile::testutil::make_zip;
use crate::*;

fn ex(z: &[u8], name: &str, req: Request) -> Result<Extract, DocError> {
    extract(z, name, &req, &Limits::default())
}

fn docx() -> Vec<u8> {
    let styles = r#"<w:styles><w:style w:styleId="Heading1"><w:name w:val="heading 1"/></w:style><w:style w:styleId="Title"><w:name w:val="Title"/></w:style></w:styles>"#;
    let doc = r#"<w:document><w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Quarterly Report</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">Revenue grew </w:t></w:r><w:ins w:id="1" w:author="a"><w:r><w:t>12%</w:t></w:r></w:ins><w:del w:id="2"><w:r><w:delText>10%</w:delText></w:r></w:del><w:r><w:footnoteReference w:id="2"/></w:r><w:r><w:commentReference w:id="0"/></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr></w:pPr><w:r><w:t>first item</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="3"/></w:numPr></w:pPr><w:r><w:t>nested item</w:t></w:r></w:p>
<w:p><w:r><w:t>Visit </w:t></w:r><w:hyperlink r:id="rId5"><w:r><w:t>our site</w:t></w:r></w:hyperlink></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Qty</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>Widget</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>7</w:t></w:r></w:p><w:p><w:r><w:t>units</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
<w:p><w:r><mc:AlternateContent><mc:Choice><w:drawing><wp:inline><wp:docPr id="1" descr="A bar chart"/></wp:inline></w:drawing></mc:Choice><mc:Fallback><w:pict><w:p><w:r><w:t>FALLBACK TEXT</w:t></w:r></w:p></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p>
<w:p><w:r><w:drawing><wp:inline><wp:docPr id="2"/></wp:inline></w:drawing></w:r></w:p>
<w:p><w:r><w:pict><v:textbox><w:txbxContent><w:p><w:r><w:t>BOXED TEXT</w:t></w:r></w:p></w:txbxContent></v:textbox></w:pict></w:r></w:p>
<w:p><w:r><m:oMath><m:r><m:t>x+y</m:t></m:r></m:oMath></w:r></w:p>
<w:p><w:pPr><w:rPr><w:ins w:id="9"/></w:rPr></w:pPr><w:r><w:t>plain after a paragraph mark flag</w:t></w:r></w:p>
</w:body></w:document>"#;
    let foot = r#"<w:footnotes><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:t>SEPARATOR</w:t></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>See the appendix.</w:t></w:r></w:p></w:footnote></w:footnotes>"#;
    let comments = r#"<w:comments><w:comment w:id="0" w:author="Dana"><w:p><w:r><w:t>Check this number</w:t></w:r></w:p></w:comment></w:comments>"#;
    let rels = r#"<Relationships><Relationship Id="rId5" Type="http://x/relationships/hyperlink" Target="https://example.com/" TargetMode="External"/></Relationships>"#;
    make_zip(&[
        ("[Content_Types].xml", b"<Types/>", 0),
        ("word/document.xml", doc.as_bytes(), 8),
        ("word/styles.xml", styles.as_bytes(), 8),
        ("word/footnotes.xml", foot.as_bytes(), 8),
        ("word/comments.xml", comments.as_bytes(), 8),
        ("word/_rels/document.xml.rels", rels.as_bytes(), 8),
        ("word/header1.xml", b"<w:hdr/>", 0),
    ])
}

#[test]
fn a_docx_reads_as_markdown_with_its_structure() {
    let e = ex(&docx(), "r.docx", Request::default()).unwrap();
    assert_eq!(e.format, Format::Docx);
    let t = &e.text;
    assert!(t.starts_with("# Quarterly Report\n\n"), "{}", t);
    assert!(t.contains("Revenue grew {+12%+}{-10%-}[^2][comment 0]"), "{}", t);
    assert!(t.contains("\n- first item\n  - nested item\n"), "{}", t);
    assert!(t.contains("Visit [our site](https://example.com/)"), "{}", t);
    assert!(t.contains("| Name | Qty |\n|---|---|\n| Widget | 7 units |"), "{}", t);
    assert!(t.contains("[image: A bar chart]") && t.contains("[equation]"), "{}", t);
    assert!(t.contains("plain after a paragraph mark flag") && !t.contains("{+plain"), "a flag in paragraph properties is not an insertion: {}", t);
    assert!(!t.contains("FALLBACK TEXT") && !t.contains("BOXED TEXT") && !t.contains("SEPARATOR"), "{}", t);
    assert!(t.contains("## Footnotes\n\n[^2]: See the appendix.") && t.contains("## Comments\n\n- **Dana** (#0): Check this number"), "{}", t);
    let loss = e.loss.join(" | ");
    assert!(loss.contains("1 image(s) without alt text") && loss.contains("1 text box(es)") && loss.contains("1 equation(s)") && loss.contains("1 header/footer"), "{}", loss);
}

#[test]
fn the_raw_parts_of_an_office_file_stay_reachable() {
    let z = docx();
    let parts = ex(&z, "r.docx", Request { member: Some(".".into()), ..Request::default() }).unwrap();
    assert!(parts.text.contains("word/document.xml") && parts.text.contains("zip archive"), "{}", parts.text);
    let raw = ex(&z, "r.docx", Request { member: Some("word/footnotes.xml".into()), ..Request::default() }).unwrap();
    assert!(raw.text.contains("<w:footnote"), "{}", raw.text);
}

fn xlsx() -> Vec<u8> {
    let wb = r#"<workbook><workbookPr/><sheets><sheet name="Sales" sheetId="1" r:id="rId1"/><sheet name="Notes" sheetId="2" state="hidden" r:id="rId2"/></sheets></workbook>"#;
    let rels = r#"<Relationships><Relationship Id="rId1" Type="x/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="x/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#;
    let sst = r#"<sst><si><t>Region</t></si><si><t>Total</t></si><si><t>North</t></si><si><t>South</t></si><si><t>Date</t></si><si><r><t>Rich </t></r><r><t>text</t></r><rPh><t>PHONETIC</t></rPh></si></sst>"#;
    let styles = r#"<styleSheet><numFmts><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts><cellXfs><xf numFmtId="0"/><xf numFmtId="164"/></cellXfs></styleSheet>"#;
    let s1 = r#"<worksheet><sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>4</v></c></row>
<row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>100</v></c><c r="C2" s="1"><v>45292</v></c></row>
<row r="3"><c r="A3" t="s"><v>3</v></c><c r="B3"><v>250.5</v></c><c r="C3" s="1"><v>45293.5</v></c></row>
<row r="4"><c r="A4" t="str"><v>Sum</v></c><c r="B4"><f>SUM(B2:B3)</f><v>350.5</v></c><c r="D4" t="b"><v>1</v></c><c r="E4" t="inlineStr"><is><t>inline</t></is></c><c r="F4" t="s"><v>5</v></c></row>
</sheetData></worksheet>"#;
    let s2 = r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>hidden note</t></is></c></row></sheetData></worksheet>"#;
    make_zip(&[
        ("xl/workbook.xml", wb.as_bytes(), 8),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes(), 8),
        ("xl/sharedStrings.xml", sst.as_bytes(), 8),
        ("xl/styles.xml", styles.as_bytes(), 8),
        ("xl/worksheets/sheet1.xml", s1.as_bytes(), 8),
        ("xl/worksheets/sheet2.xml", s2.as_bytes(), 8),
        ("xl/charts/chart1.xml", b"<c/>", 0),
        ("xl/vbaProject.bin", b"\x00", 0),
    ])
}

#[test]
fn a_workbook_summarises_each_sheet_with_typed_cells() {
    let e = ex(&xlsx(), "b.xlsx", Request::default()).unwrap();
    assert_eq!((e.format, e.unit, e.total_units), (Format::Xlsx, Unit::Sheet, Some(2)));
    let t = &e.text;
    assert!(t.contains("Workbook with 2 sheets: 1: Sales, 2: Notes"), "{}", t);
    assert!(t.contains("## Sheet 1: Sales (4 rows × 6 columns · last cell F4)"), "{}", t);
    assert!(t.contains("| 2 | North | 100 | 2024-01-01 |"), "dates come from the number format: {}", t);
    assert!(t.contains("| 3 | South | 250.5 | 2024-01-02 12:00:00 |"), "{}", t);
    assert!(t.contains("Sum | 350.5 (=SUM(B2:B3)) |") && t.contains("TRUE") && t.contains("inline") && t.contains("Rich text") && !t.contains("PHONETIC"), "{}", t);
    assert!(t.contains("## Sheet 2: Notes (1 row × 1 column · last cell A1 · hidden)"), "{}", t);
    let loss = e.loss.join(" | ");
    assert!(loss.contains("1 chart(s)") && loss.contains("macros"), "{}", loss);
}

#[test]
fn a_sheet_and_range_select_a_slice_and_bad_selections_say_what_exists() {
    let r = |sheet: Option<&str>, range: Option<&str>| ex(&xlsx(), "b.xlsx", Request { sheet: sheet.map(String::from), range: range.map(String::from), ..Request::default() });
    let e = r(Some("sales"), Some("A3:B3")).unwrap();
    assert!(e.text.contains("Header (row 1): A=Region, B=Total") && e.text.contains("| 3 | South | 250.5 |") && !e.text.contains("| 2 |"), "{}", e.text);
    assert!(r(Some("2"), None).unwrap().text.contains("hidden note"));
    assert!(r(Some("Nope"), None).unwrap_err().0.contains("1: Sales, 2: Notes"));
    assert!(r(None, Some("A1:B2")).unwrap_err().0.contains("name a sheet"));
    assert!(r(Some("Sales"), Some("??")).unwrap_err().0.contains("not a range"));
}

fn pptx() -> Vec<u8> {
    let pres = r#"<p:presentation><p:sldIdLst><p:sldId id="256" r:id="rId1"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>"#;
    let prels = r#"<Relationships><Relationship Id="rId1" Type="x/slide" Target="slides/slide1.xml"/><Relationship Id="rId2" Type="x/slide" Target="slides/slide2.xml"/></Relationships>"#;
    let s1 = r#"<p:sld><p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Road</a:t></a:r><a:r><a:t>map</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:cNvPr id="3" name="Body"/><p:cNvSpPr/><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Ship R2</a:t></a:r></a:p><a:p><a:pPr lvl="1"/><a:r><a:t>docx reader</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:cNvPr id="4" name="Num"/><p:cNvSpPr/><p:nvPr><p:ph type="sldNum"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:fld><a:t>1</a:t></a:fld></a:p></p:txBody></p:sp>
<p:pic><p:nvPicPr><p:cNvPr id="5" name="Pic" descr="Team photo"/></p:nvPicPr></p:pic>
<p:pic><p:nvPicPr><p:cNvPr id="6" name="Pic2"/></p:nvPicPr></p:pic>
</p:spTree></p:cSld></p:sld>"#;
    let s1rels = r#"<Relationships><Relationship Id="rId9" Type="http://x/relationships/notesSlide" Target="../notesSlides/notesSlide1.xml"/></Relationships>"#;
    let notes = r#"<p:notes><p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Img"/><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr></p:sp>
<p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes"/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Mention the demo</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:cNvPr id="4" name="Num"/><p:nvPr><p:ph type="sldNum"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:fld><a:t>1</a:t></a:fld></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:notes>"#;
    let s2 = r#"<p:sld><p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="T"/><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Numbers</a:t></a:r></a:p></p:txBody></p:sp>
<p:graphicFrame><a:graphic><a:graphicData><a:tbl>
<a:tr><a:tc><a:txBody><a:p><a:r><a:t>Q</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>Rev</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
<a:tr><a:tc><a:txBody><a:p><a:r><a:t>1</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>10</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
</a:tbl></a:graphicData></a:graphic></p:graphicFrame>
</p:spTree></p:cSld></p:sld>"#;
    make_zip(&[
        ("ppt/presentation.xml", pres.as_bytes(), 8),
        ("ppt/_rels/presentation.xml.rels", prels.as_bytes(), 8),
        ("ppt/slides/slide1.xml", s1.as_bytes(), 8),
        ("ppt/slides/_rels/slide1.xml.rels", s1rels.as_bytes(), 8),
        ("ppt/slides/slide2.xml", s2.as_bytes(), 8),
        ("ppt/notesSlides/notesSlide1.xml", notes.as_bytes(), 8),
    ])
}

#[test]
fn a_deck_reads_slide_by_slide_with_notes_tables_and_alt_text() {
    let e = ex(&pptx(), "d.pptx", Request::default()).unwrap();
    assert_eq!((e.format, e.unit, e.total_units), (Format::Pptx, Unit::Slide, Some(2)));
    let t = &e.text;
    assert!(t.contains("## Slide 1: Roadmap\n\nShip R2\n- docx reader\n[image: Team photo]"), "{}", t);
    assert!(t.contains("Notes: Mention the demo") && !t.contains("\n1\n"), "slide numbers are chrome: {}", t);
    assert!(t.contains("## Slide 2: Numbers\n\n| Q | Rev |\n|---|---|\n| 1 | 10 |"), "{}", t);
    assert!(e.loss.iter().any(|l| l.contains("1 picture(s) without alt text")), "{:?}", e.loss);
    let p = ex(&pptx(), "d.pptx", Request { start: Some(2), end: Some(2), ..Request::default() }).unwrap();
    assert!(p.text.contains("## Slide 2") && !p.text.contains("## Slide 1") && !p.text.contains("shown"), "{}", p.text);
    let first = ex(&pptx(), "d.pptx", Request { start: Some(1), end: Some(1), ..Request::default() }).unwrap();
    assert!(first.text.contains("(slides 1-1 of 2 shown; pass start/end to page)"), "{}", first.text);
    assert!(ex(&pptx(), "d.pptx", Request { start: Some(9), ..Request::default() }).unwrap_err().0.contains("2 slides"));
}

#[test]
fn a_zip_that_is_not_an_office_file_is_still_a_plain_archive() {
    let z = make_zip(&[("notes.txt", b"hi", 0)]);
    assert!(ex(&z, "a.zip", Request::default()).unwrap().text.contains("zip archive"));
}

#[test]
fn damaged_and_hostile_office_parts_give_partial_output_and_never_panic() {
    // An unterminated document keeps what came before the damage and says so.
    let z = make_zip(&[("word/document.xml", b"<w:document><w:body><w:p><w:r><w:t>kept</w:t></w:r></w:p><w:p><w:r><w:t", 0)]);
    let e = ex(&z, "a.docx", Request::default()).unwrap();
    assert!(e.text.contains("kept") && e.loss.iter().any(|l| l.contains("stops early")), "{:?} {}", e.loss, e.text);
    // Deep nesting stops cleanly.
    let deep = "<w:p>".repeat(400);
    let z = make_zip(&[("word/document.xml", deep.as_bytes(), 0)]);
    assert!(ex(&z, "a.docx", Request::default()).unwrap().loss.iter().any(|l| l.contains("nested more than")));
    // A part that inflates past its limit is refused, not read.
    let z = make_zip(&[("word/document.xml", &vec![b' '; 8 << 20], 8)]);
    assert!(ex(&z, "a.docx", Request::default()).unwrap_err().0.contains("limit"));
    // Workbook pointing at missing or malformed parts.
    let wb = br#"<workbook><sheets><sheet name="S" r:id="rId1"/></sheets></workbook>"#;
    let rels = br#"<Relationships><Relationship Id="rId1" Target="worksheets/missing.xml"/></Relationships>"#;
    let z = make_zip(&[("xl/workbook.xml", wb, 0), ("xl/_rels/workbook.xml.rels", rels, 0)]);
    assert!(ex(&z, "a.xlsx", Request::default()).unwrap_err().0.contains("missing part"));
    // A relationship cannot reach above the package root.
    let rels = br#"<Relationships><Relationship Id="rId1" Target="../../../../etc/passwd"/></Relationships>"#;
    let z = make_zip(&[("xl/workbook.xml", wb, 0), ("xl/_rels/workbook.xml.rels", rels, 0)]);
    assert!(ex(&z, "a.xlsx", Request::default()).unwrap_err().0.contains("missing part"));
}

#[test]
fn bit_flipped_office_files_never_panic() {
    let mut x: u64 = 0xdead_beef_cafe_f00d;
    for z in [docx(), xlsx(), pptx()] {
        for _ in 0..400 {
            let mut f = z.clone();
            for _ in 0..4 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let at = (x as usize) % f.len();
                f[at] ^= (x >> 40) as u8 | 1;
            }
            let _ = extract(&f, "x.docx", &Request::default(), &Limits::default());
        }
    }
}
