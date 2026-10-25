//! PDF reading against small hand-built files: one fixture per feature (`specs/014` §9).

use crate::*;
use std::io::Write;

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn a85(data: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    for chunk in data.chunks(4) {
        let mut v = [0u8; 4];
        v[..chunk.len()].copy_from_slice(chunk);
        let mut n = u32::from_be_bytes(v);
        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = (n % 85) as u8 + b'!';
            n /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

fn hex(data: &[u8]) -> Vec<u8> {
    let mut s: String = data.iter().map(|b| format!("{:02X}", b)).collect();
    s.push('>');
    s.into_bytes()
}

#[derive(Default)]
struct B {
    objs: Vec<(u32, Vec<u8>)>,
    trailer_extra: String,
}

impl B {
    fn obj(&mut self, n: u32, body: &str) -> &mut B {
        self.objs.push((n, body.as_bytes().to_vec()));
        self
    }
    fn stream(&mut self, n: u32, extra: &str, data: &[u8]) -> &mut B {
        let mut v = format!("<< /Length {} {} >>\nstream\n", data.len(), extra).into_bytes();
        v.extend_from_slice(data);
        v.extend_from_slice(b"\nendstream");
        self.objs.push((n, v));
        self
    }
    fn build(&self) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offs = std::collections::BTreeMap::new();
        for (n, body) in &self.objs {
            offs.insert(*n, out.len());
            out.extend_from_slice(format!("{} 0 obj\n", n).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let max = *offs.keys().max().unwrap_or(&0);
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
        for i in 1..=max {
            match offs.get(&i) {
                Some(o) => out.extend_from_slice(format!("{:010} 00000 n \n", o).as_bytes()),
                None => out.extend_from_slice(b"0000000000 65535 f \n"),
            }
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R {} >>\nstartxref\n{}\n%%EOF\n", max + 1, self.trailer_extra, xref).as_bytes());
        out
    }
}

const FONT: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";

/// One page, Helvetica in /F1, the given content stream.
fn one_page(content: &[u8], stream_extra: &str) -> B {
    let mut b = B::default();
    b.obj(1, "<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
        .obj(3, "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>")
        .stream(4, stream_extra, content)
        .obj(5, FONT);
    b
}

fn read(pdf: &[u8], req: Request) -> Result<Extract, DocError> {
    extract(pdf, "t.pdf", &req, &Limits::default())
}

fn text_of(content: &str) -> String {
    let e = read(&one_page(content.as_bytes(), "").build(), Request::default()).unwrap();
    e.text.split("## Page 1\n\n").nth(1).unwrap_or("").trim().to_string()
}

#[test]
fn lines_words_and_spacing_come_from_positions() {
    assert_eq!(text_of("BT /F1 12 Tf 72 700 Td (Hello World) Tj 0 -14 Td (Second line) Tj ET"), "Hello World\nSecond line");
    assert_eq!(text_of("BT /F1 12 Tf 72 700 Td [(Hel) -20 (lo) -300 (World)] TJ ET"), "Hello World", "a large kern is a space, a small one is not");
    assert_eq!(text_of("BT /F1 12 Tf 72 700 Td (Hello) Tj 80 0 Td (World) Tj ET"), "Hello World");
    assert_eq!(text_of("BT /F1 12 Tf 72 700 Td (Hello) Tj 30 0 Td (World) Tj ET"), "HelloWorld", "abutting runs stay joined");
    assert_eq!(text_of("BT /F1 12 Tf 14 TL 72 700 Td (one) ' (two) ' ET"), "one\ntwo");
    assert_eq!(text_of("BT /F1 12 Tf 1 0 0 1 72 700 Tm (A) Tj 1 0 0 1 72 650 Tm (B) Tj ET"), "A\nB");
    assert_eq!(text_of("1 0 0 1 0 0 cm q 2 0 0 2 0 0 cm BT /F1 12 Tf 72 700 Td (Scaled) Tj ET Q"), "Scaled");
}

#[test]
fn string_escapes_hex_strings_and_encodings() {
    assert_eq!(text_of(r"BT /F1 12 Tf (a\(b\)\\ \101\n) Tj ET"), "a(b)\\ A");
    assert_eq!(text_of("BT /F1 12 Tf <48656C6C6F> Tj ET"), "Hello");
    assert_eq!(text_of("BT /F1 12 Tf (caf\\351 \\223q\\224) Tj ET"), "café \u{201c}q\u{201d}", "WinAnsi codes");
    assert_eq!(text_of("BT /F1 12 Tf (\\256\\231) Tj ET"), "®™");
}

#[test]
fn a_differences_array_and_a_mac_roman_base_remap_codes() {
    let mut b = one_page(b"BT /F1 12 Tf (AB) Tj ET", "");
    b.obj(5, "<< /Type /Font /Subtype /Type1 /BaseFont /X /Encoding << /Type /Encoding /Differences [65 /Euro /eacute] >> >>");
    let e = read(&b.build(), Request::default()).unwrap();
    assert!(e.text.contains("€é"), "{}", e.text);
    let mut b = one_page(b"BT /F1 12 Tf (\\216) Tj ET", "");
    b.obj(5, "<< /Type /Font /Subtype /Type1 /BaseFont /X /Encoding /MacRomanEncoding >>");
    assert!(read(&b.build(), Request::default()).unwrap().text.contains('é'));
}

#[test]
fn stream_filters_decode_alone_and_chained() {
    let c = b"BT /F1 12 Tf 72 700 Td (Filtered text) Tj ET";
    for (extra, data) in [
        ("/Filter /FlateDecode", zlib(c)),
        ("/Filter /ASCIIHexDecode", hex(c)),
        ("/Filter /ASCII85Decode", a85(c)),
        ("/Filter [/ASCIIHexDecode /FlateDecode]", hex(&zlib(c))),
        ("/Filter [/ASCII85Decode /FlateDecode]", a85(&zlib(c))),
        ("/Filter /RunLengthDecode", [&[(c.len() - 1) as u8][..], c, &[128]].concat()),
    ] {
        let e = read(&one_page(&data, extra).build(), Request::default()).unwrap();
        assert!(e.text.contains("Filtered text"), "{}: {}", extra, e.text);
    }
}

#[test]
fn a_png_predictor_is_undone() {
    // Two rows of three bytes with the Up filter: the second row adds the first.
    let raw = [2u8, 1, 2, 3, 2, 1, 1, 1];
    let doc = {
        let mut b = B::default();
        b.stream(9, "/Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 3 >>", &zlib(&raw));
        b.build()
    };
    let limits = Limits::default();
    let d = crate::pdf::Doc::open(&doc, &limits).unwrap();
    let o = d.get(9).unwrap();
    let crate::pdf::Obj::Stream(s) = &*o else { panic!("not a stream") };
    assert_eq!(d.decode(s).unwrap().0, [1, 2, 3, 2, 3, 4]);
}

#[test]
fn tounicode_maps_a_simple_font_and_a_cid_font() {
    let cmap = b"/CIDInit begincmap 1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfrange <01> <03> <0041> endbfrange endcmap";
    let mut b = one_page(b"BT /F1 12 Tf (\\001\\002\\003) Tj ET", "");
    b.obj(5, "<< /Type /Font /Subtype /Type1 /BaseFont /Sub /ToUnicode 6 0 R >>").stream(6, "", cmap);
    assert!(read(&b.build(), Request::default()).unwrap().text.contains("ABC"));

    let cid_map = b"1 begincodespacerange <0000> <FFFF> endcodespacerange 2 beginbfchar <0001> <0048> <0002> <0069> endbfchar";
    let mut b = one_page(b"BT /F1 12 Tf <00010002> Tj ET", "");
    b.obj(5, "<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H /DescendantFonts [7 0 R] /ToUnicode 6 0 R >>")
        .stream(6, "", cid_map)
        .obj(7, "<< /Type /Font /Subtype /CIDFontType2 /DW 600 /W [1 [500 700]] >>");
    let e = read(&b.build(), Request::default()).unwrap();
    assert!(e.text.contains("\nHi\n"), "{}", e.text);
    // The same font without a Unicode map: nothing is guessed, and the report says so.
    let mut b = one_page(b"BT /F1 12 Tf <00010002> Tj ET", "");
    b.obj(5, "<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H /DescendantFonts [7 0 R] >>").obj(7, "<< /Type /Font /Subtype /CIDFontType2 >>");
    let e = read(&b.build(), Request::default()).unwrap();
    assert!(e.text.contains("(no text)") && e.loss.iter().any(|l| l.contains("2 character(s) on page(s) 1 use fonts with no Unicode map")), "{:?}", e.loss);
}

fn three_pages() -> Vec<u8> {
    let mut b = B::default();
    b.obj(1, "<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /Resources << /Font << /F1 9 0 R >> >> >>")
        .obj(3, "<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>")
        .obj(4, "<< /Type /Page /Parent 2 0 R /Contents 7 0 R /Resources << /XObject << /Im1 10 0 R >> >> >>")
        .obj(5, "<< /Type /Page /Parent 2 0 R /Contents 8 0 R >>")
        .stream(6, "", b"BT /F1 12 Tf 72 700 Td (First page) Tj ET")
        .stream(7, "", b"q 100 0 0 100 0 0 cm /Im1 Do Q")
        .stream(8, "", b"BT /F1 12 Tf 72 700 Td (Third page) Tj ET")
        .obj(9, FONT)
        .stream(10, "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", b"\x00");
    b.build()
}

#[test]
fn pages_are_ordered_paged_and_image_only_pages_are_reported() {
    let e = read(&three_pages(), Request::default()).unwrap();
    assert_eq!((e.format, e.unit, e.total_units), (Format::Pdf, Unit::Page, Some(3)));
    assert!(e.text.starts_with("PDF · 3 pages\n\n## Page 1\n\nFirst page\n\n## Page 2\n\n(no text)\n\n## Page 3\n\nThird page"), "{}", e.text);
    let loss = e.loss.join(" | ");
    assert!(loss.contains("page(s) 2: no text layer") && loss.contains("1 image(s) not read"), "{}", loss);
    let p = read(&three_pages(), Request { start: Some(3), end: Some(3), ..Request::default() }).unwrap();
    assert!(p.text.contains("## Page 3") && !p.text.contains("First page") && !p.text.contains("shown"), "{}", p.text);
    let one = read(&three_pages(), Request { start: Some(1), end: Some(1), ..Request::default() }).unwrap();
    assert!(one.text.contains("(pages 1-1 of 3 shown; pass start/end to page)"), "{}", one.text);
    assert!(read(&three_pages(), Request { start: Some(9), ..Request::default() }).unwrap_err().0.contains("3 pages"));
}

#[test]
fn form_xobjects_are_read_and_a_self_referencing_form_ends() {
    let mut b = one_page(b"/Fm1 Do", "");
    b.obj(3, "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> /XObject << /Fm1 6 0 R >> >> /Contents 4 0 R >>")
        .stream(6, "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Matrix [1 0 0 1 10 20] /Resources << /Font << /F1 5 0 R >> /XObject << /Fm1 6 0 R >> >>", b"BT /F1 12 Tf (In a form) Tj ET /Fm1 Do");
    let e = read(&b.build(), Request::default()).unwrap();
    assert!(e.text.contains("In a form"), "{}", e.text);
    assert_eq!(e.text.matches("In a form").count(), 1, "the cycle is cut");
}

#[test]
fn inline_images_do_not_leak_their_bytes_into_the_text() {
    let mut c = b"BT /F1 12 Tf (before) Tj ET q 10 0 0 10 0 0 cm BI /W 2 /H 2 /BPC 8 /CS /G ID ".to_vec();
    c.extend_from_slice(&[b'T', b'j', b' ', b'(', b')', 0, 255, 7]);
    c.extend_from_slice(b"\nEI Q BT /F1 12 Tf 0 -20 Td (after) Tj ET");
    let e = read(&one_page(&c, "").build(), Request::default()).unwrap();
    assert!(e.text.contains("before") && e.text.contains("after") && e.loss.iter().any(|l| l.contains("1 image(s)")), "{}", e.text);
}

#[test]
fn metadata_titles_are_shown_and_utf16_is_decoded() {
    let mut b = one_page(b"BT /F1 12 Tf (x) Tj ET", "");
    b.obj(9, "<< /Title (My Document) /Author <FEFF0041006C0069> >>");
    b.trailer_extra = "/Info 9 0 R".into();
    let e = read(&b.build(), Request::default()).unwrap();
    assert!(e.text.starts_with("PDF · 1 page · Title: My Document · Author: Ali\n"), "{}", e.text);
}

#[test]
fn indirect_and_wrong_stream_lengths_are_survivable() {
    let c = b"BT /F1 12 Tf (Length trouble) Tj ET";
    let mut b = one_page(c, "");
    b.objs.retain(|(n, _)| *n != 4);
    let mut s = b"<< /Length 8 0 R >>\nstream\n".to_vec();
    s.extend_from_slice(c);
    s.extend_from_slice(b"\nendstream");
    b.objs.push((4, s));
    b.obj(8, &c.len().to_string());
    assert!(read(&b.build(), Request::default()).unwrap().text.contains("Length trouble"));
    let mut b = one_page(c, "");
    b.objs.retain(|(n, _)| *n != 4);
    let mut s = b"<< /Length 5 >>\nstream\n".to_vec();
    s.extend_from_slice(c);
    s.extend_from_slice(b"\nendstream");
    b.objs.push((4, s));
    assert!(read(&b.build(), Request::default()).unwrap().text.contains("Length trouble"), "a wrong /Length falls back to endstream");
}

/// PDF 1.5 layout: objects 1-3 in an object stream, a cross-reference stream, no `trailer` keyword.
fn compressed_layout() -> Vec<u8> {
    let bodies = ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>", "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"];
    let mut header = String::new();
    let mut body = String::new();
    for (i, b) in bodies.iter().enumerate() {
        header.push_str(&format!("{} {} ", i + 1, body.len()));
        body.push_str(b);
        body.push(' ');
    }
    let first = header.len();
    let objstm = zlib(format!("{}{}", header, body).as_bytes());
    let mut out = b"%PDF-1.5\n".to_vec();
    let mut offs = [0usize; 8];
    let put = |out: &mut Vec<u8>, n: usize, dict: &str, data: &[u8], offs: &mut [usize; 8]| {
        offs[n] = out.len();
        out.extend_from_slice(format!("{} 0 obj\n<< /Length {} {} >>\nstream\n", n, data.len(), dict).as_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream\nendobj\n");
    };
    put(&mut out, 4, "", b"BT /F1 12 Tf 72 700 Td (From a compressed layout) Tj ET", &mut offs);
    offs[5] = out.len();
    out.extend_from_slice(format!("5 0 obj\n{}\nendobj\n", FONT).as_bytes());
    put(&mut out, 6, &format!("/Type /ObjStm /N 3 /First {} /Filter /FlateDecode", first), &objstm, &mut offs);
    let mut rows = vec![];
    let mut entry = |t: u8, a: u32, b: u16| {
        rows.push(t);
        rows.extend_from_slice(&a.to_be_bytes());
        rows.extend_from_slice(&b.to_be_bytes());
    };
    entry(0, 0, 65535);
    entry(2, 6, 0);
    entry(2, 6, 1);
    entry(2, 6, 2);
    entry(1, offs[4] as u32, 0);
    entry(1, offs[5] as u32, 0);
    entry(1, offs[6] as u32, 0);
    let xref_off = out.len();
    entry_self(&mut rows, xref_off);
    put(&mut out, 7, "/Type /XRef /Size 8 /W [1 4 2] /Root 1 0 R", &rows, &mut offs);
    out.extend_from_slice(format!("startxref\n{}\n%%EOF\n", xref_off).as_bytes());
    out
}

fn entry_self(rows: &mut Vec<u8>, off: usize) {
    rows.push(1);
    rows.extend_from_slice(&(off as u32).to_be_bytes());
    rows.extend_from_slice(&0u16.to_be_bytes());
}

#[test]
fn a_cross_reference_stream_with_an_object_stream_is_read() {
    let e = read(&compressed_layout(), Request::default()).unwrap();
    assert!(e.text.contains("From a compressed layout"), "{}", e.text);
    assert!(!e.loss.iter().any(|l| l.contains("rebuilt")), "{:?}", e.loss);
}

#[test]
fn a_damaged_or_missing_cross_reference_table_is_rebuilt_and_reported() {
    let good = one_page(b"BT /F1 12 Tf (Recovered) Tj ET", "").build();
    let text = String::from_utf8_lossy(&good).into_owned();
    // wrong startxref
    let bad = text.replace("startxref\n", "startxref\n9");
    let e = read(bad.as_bytes(), Request::default()).unwrap();
    assert!(e.text.contains("Recovered") && e.loss.iter().any(|l| l.contains("rebuilt")), "{:?}", e.loss);
    // cut off before the table and trailer: the catalog is found by scanning
    let cut = &good[..text.find("xref\n0 ").unwrap()];
    let e = read(cut, Request::default()).unwrap();
    assert!(e.text.contains("Recovered"), "{}", e.text);
    // the compressed layout loses its xref stream: objects inside the object stream are still found
    let c = compressed_layout();
    let t = String::from_utf8_lossy(&c).into_owned();
    let cut = &c[..t.rfind("7 0 obj").unwrap()];
    let e = read(cut, Request::default()).unwrap();
    assert!(e.text.contains("From a compressed layout"), "{}", e.text);
}

#[test]
fn encrypted_files_are_refused_not_misread() {
    let mut b = one_page(b"BT /F1 12 Tf (secret) Tj ET", "");
    b.obj(9, "<< /Filter /Standard /V 4 /R 4 >>");
    b.trailer_extra = "/Encrypt 9 0 R".into();
    let e = read(&b.build(), Request::default()).unwrap_err();
    assert!(e.0.contains("encrypted"), "{}", e);
}

#[test]
fn a_cyclic_page_tree_ends() {
    let mut b = B::default();
    b.obj(1, "<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, "<< /Type /Pages /Kids [2 0 R 3 0 R] /Count 2 >>")
        .obj(3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>")
        .stream(4, "", b"BT /F1 12 Tf (Still here) Tj ET")
        .obj(5, FONT);
    let e = read(&b.build(), Request::default()).unwrap();
    assert!(e.text.contains("Still here") && e.total_units == Some(1), "{}", e.text);
}

#[test]
fn a_content_bomb_is_refused_and_the_rest_of_the_document_is_still_read() {
    let mut b = B::default();
    b.obj(1, "<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /Resources << /Font << /F1 9 0 R >> >> >>")
        .obj(3, "<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>")
        .obj(4, "<< /Type /Page /Parent 2 0 R /Contents 7 0 R >>")
        .stream(6, "/Filter /FlateDecode", &zlib(&vec![b' '; 40 << 20]))
        .stream(7, "", b"BT /F1 12 Tf (Second page) Tj ET")
        .obj(9, FONT);
    let e = read(&b.build(), Request::default()).unwrap();
    assert!(e.text.contains("Second page"), "{}", e.text);
    assert!(e.loss.iter().any(|l| l.contains("page 1") && l.contains("could not be decoded") && l.contains("bomb")), "{:?}", e.loss);
}

#[test]
fn damaged_files_never_panic() {
    let samples = [three_pages(), compressed_layout(), one_page(b"BT /F1 12 Tf (x) Tj [(a) -300 (b)] TJ ET", "").build()];
    for z in &samples {
        for cut in (0..z.len()).step_by(7) {
            let _ = crate::pdftext::extract(&z[..cut], &Request::default(), &Limits::default());
        }
    }
    let mut x: u64 = 0xfeed_face_dead_beef;
    for z in &samples {
        for _ in 0..600 {
            let mut f = z.clone();
            for _ in 0..4 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let at = (x as usize) % f.len();
                f[at] = (x >> 40) as u8;
            }
            let _ = crate::pdftext::extract(&f, &Request::default(), &Limits::default());
        }
    }
}

#[test]
fn pdf_syntax_junk_never_panics() {
    let alphabet: &[u8] = b"<<>>[]()/ 0123456789 obj endobj stream endstream R xref trailer startxref \n%";
    let mut x: u64 = 0x1357_9bdf_2468_ace0;
    for n in 0..400usize {
        let mut f = b"%PDF-1.4\n".to_vec();
        for _ in 0..(n % 300) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            f.push(alphabet[(x as usize) % alphabet.len()]);
        }
        let _ = crate::pdftext::extract(&f, &Request::default(), &Limits::default());
    }
}
