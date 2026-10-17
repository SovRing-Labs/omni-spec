#!/usr/bin/env bash
# Builds tests/data/ooxml/*.{xlsx,docx} with LibreOffice (black-box producer, `soffice --headless --convert-to`).
# Every source document below is our own text; the generated files are committed as fixtures (program output carries
# no third-party licence). Re-run only to regenerate. Needs soffice + python3 (PNG for the image fixture).
set -euo pipefail
cd "$(dirname "$0")/.."
out=$PWD/tests/data/ooxml
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
LO=(soffice "-env:UserInstallation=file://$tmp/profile" --headless)

# ---- spreadsheet 1: CSV (type detection: numbers, ISO dates, booleans)
cat > "$tmp/basic.csv" <<'EOF'
Name,Qty,Price,Date,Flag
Widget,3,4.5,2026-10-06,TRUE
Gadget,10,0.25,2025-01-31,FALSE
"Comma, quoted",-7,1e3,1999-12-31,TRUE
EOF

# ---- spreadsheet 2: flat ODS with two sheets, formulas, an error, formats, merged cells
cat > "$tmp/rich.fods" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
 xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
 xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:number="urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0"
 xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2"
 office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.spreadsheet">
 <office:automatic-styles>
  <number:date-style style:name="N1"><number:day number:style="long"/><number:text>.</number:text><number:month number:style="long"/><number:text>.</number:text><number:year number:style="long"/></number:date-style>
  <number:date-style style:name="N2"><number:year number:style="long"/><number:text>-</number:text><number:month number:style="long"/><number:text>-</number:text><number:day number:style="long"/><number:text> </number:text><number:hours number:style="long"/><number:text>:</number:text><number:minutes number:style="long"/></number:date-style>
  <number:percentage-style style:name="N3"><number:number number:decimal-places="1" number:min-integer-digits="1"/><number:text>%</number:text></number:percentage-style>
  <number:time-style style:name="N4"><number:hours number:style="long"/><number:text>:</number:text><number:minutes number:style="long"/><number:text>:</number:text><number:seconds number:style="long"/></number:time-style>
  <style:style style:name="ce1" style:family="table-cell" style:data-style-name="N1"/>
  <style:style style:name="ce2" style:family="table-cell" style:data-style-name="N2"/>
  <style:style style:name="ce3" style:family="table-cell" style:data-style-name="N3"/>
  <style:style style:name="ce4" style:family="table-cell" style:data-style-name="N4"/>
 </office:automatic-styles>
 <office:body><office:spreadsheet>
  <table:table table:name="Items">
   <table:table-row>
    <table:table-cell office:value-type="string"><text:p>SKU</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Title</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Qty</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Unit cost</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Total</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Bought</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Checked</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Margin</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Ratio</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Ok</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Time</text:p></table:table-cell>
   </table:table-row>
   <table:table-row>
    <table:table-cell office:value-type="string"><text:p>A-001</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Dell Latitude 7490 &amp; dock</text:p></table:table-cell>
    <table:table-cell office:value-type="float" office:value="2"><text:p>2</text:p></table:table-cell>
    <table:table-cell office:value-type="float" office:value="85.5"><text:p>85.5</text:p></table:table-cell>
    <table:table-cell table:formula="of:=[.C2]*[.D2]" office:value-type="float" office:value="171"><text:p>171</text:p></table:table-cell>
    <table:table-cell table:style-name="ce1" office:value-type="date" office:date-value="2026-10-06"><text:p>06.10.2026</text:p></table:table-cell>
    <table:table-cell table:style-name="ce2" office:value-type="date" office:date-value="2026-10-06T14:30:00"><text:p>2026-10-06 14:30</text:p></table:table-cell>
    <table:table-cell table:style-name="ce3" office:value-type="percentage" office:value="0.255"><text:p>25.5%</text:p></table:table-cell>
    <table:table-cell table:formula="of:=1/0" office:value-type="float" office:value="0"><text:p>#DIV/0!</text:p></table:table-cell>
    <table:table-cell office:value-type="boolean" office:boolean-value="true"><text:p>TRUE</text:p></table:table-cell>
    <table:table-cell table:style-name="ce4" office:value-type="time" office:time-value="PT13H05M09S"><text:p>13:05:09</text:p></table:table-cell>
   </table:table-row>
   <table:table-row>
    <table:table-cell office:value-type="string"><text:p>A-002</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Ünïcödé — 日本語 ✓</text:p></table:table-cell>
    <table:table-cell office:value-type="float" office:value="1"><text:p>1</text:p></table:table-cell>
    <table:table-cell office:value-type="float" office:value="0.1"><text:p>0.1</text:p></table:table-cell>
    <table:table-cell table:formula="of:=[.C3]*[.D3]" office:value-type="float" office:value="0.1"><text:p>0.1</text:p></table:table-cell>
    <table:table-cell table:style-name="ce1" office:value-type="date" office:date-value="1900-03-01"><text:p>01.03.1900</text:p></table:table-cell>
    <table:table-cell/>
    <table:table-cell office:value-type="string"><text:p>  spaced  </text:p></table:table-cell>
    <table:table-cell table:formula="of:=CONCATENATE([.A3];&quot;-x&quot;)" office:value-type="string" office:string-value="A-002-x"><text:p>A-002-x</text:p></table:table-cell>
    <table:table-cell office:value-type="boolean" office:boolean-value="false"><text:p>FALSE</text:p></table:table-cell>
   </table:table-row>
   <table:table-row table:number-rows-repeated="2"><table:table-cell table:number-columns-repeated="11"/></table:table-row>
   <table:table-row>
    <table:table-cell table:number-columns-spanned="3" table:number-rows-spanned="1" office:value-type="string"><text:p>Merged note</text:p></table:table-cell>
    <table:covered-table-cell table:number-columns-repeated="2"/>
    <table:table-cell office:value-type="float" office:value="12345678901"><text:p>12345678901</text:p></table:table-cell>
   </table:table-row>
  </table:table>
  <table:table table:name="Notes &amp; Ünïcode">
   <table:table-row>
    <table:table-cell office:value-type="string"><text:p>line one</text:p><text:p>line two</text:p></table:table-cell>
    <table:table-cell office:value-type="float" office:value="-3.25"><text:p>-3.25</text:p></table:table-cell>
   </table:table-row>
  </table:table>
 </office:spreadsheet></office:body>
</office:document>
EOF

# ---- document: flat ODT with headings, styles, nested lists, a table with merged cells, an image, links, a footnote
python3 - "$tmp/red.png" <<'PY'
import struct, sys, zlib
w, h = 20, 10
raw = b"".join(b"\x00" + b"\xff\x00\x00" * w for _ in range(h))
def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
PY
img64=$(base64 -w0 "$tmp/red.png")
cat > "$tmp/doc.fodt" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
 xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
 xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
 xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink"
 xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0"
 office:version="1.3" office:mimetype="application/vnd.oasis.opendocument.text">
 <office:meta><dc:title>Fixture Document</dc:title><meta:initial-creator>omni-spec fixtures</meta:initial-creator></office:meta>
 <office:font-face-decls><style:font-face style:name="Liberation Mono" svg:font-family="'Liberation Mono'" style:font-family-generic="modern" style:font-pitch="fixed"/></office:font-face-decls>
 <office:styles>
  <style:style style:name="Standard" style:family="paragraph" style:class="text"/>
  <style:style style:name="Heading" style:family="paragraph" style:parent-style-name="Standard" style:class="text"/>
  <style:style style:name="Heading_20_1" style:display-name="Heading 1" style:family="paragraph" style:parent-style-name="Heading" style:default-outline-level="1" style:class="text"><style:text-properties fo:font-size="130%" fo:font-weight="bold"/></style:style>
  <style:style style:name="Heading_20_2" style:display-name="Heading 2" style:family="paragraph" style:parent-style-name="Heading" style:default-outline-level="2" style:class="text"><style:text-properties fo:font-size="115%" fo:font-weight="bold"/></style:style>
  <style:style style:name="Heading_20_3" style:display-name="Heading 3" style:family="paragraph" style:parent-style-name="Heading" style:default-outline-level="3" style:class="text"><style:text-properties fo:font-size="101%" fo:font-weight="bold"/></style:style>
  <style:style style:name="Preformatted_20_Text" style:display-name="Preformatted Text" style:family="paragraph" style:parent-style-name="Standard" style:class="html"><style:text-properties style:font-name="Liberation Mono"/></style:style>
  <style:style style:name="Quotations" style:family="paragraph" style:parent-style-name="Standard" style:class="html"><style:paragraph-properties fo:margin-left="0.4in"/></style:style>
  <style:style style:name="Source_20_Text" style:display-name="Source Text" style:family="text"><style:text-properties style:font-name="Liberation Mono"/></style:style>
 </office:styles>
 <office:automatic-styles>
  <style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
  <style:style style:name="T2" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>
  <style:style style:name="T3" style:family="text"><style:text-properties style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color"/></style:style>
  <style:style style:name="T4" style:family="text"><style:text-properties style:text-line-through-style="solid"/></style:style>
  <style:style style:name="T5" style:family="text"><style:text-properties style:font-name="Liberation Mono" fo:font-family="'Liberation Mono'" style:font-family-generic="modern" style:font-pitch="fixed"/></style:style>
  <style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:break-before="page"/></style:style>
  <text:list-style style:name="L1">
   <text:list-level-style-bullet text:level="1" text:bullet-char="•"><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" fo:text-indent="-0.25in" fo:margin-left="0.5in"/></style:list-level-properties></text:list-level-style-bullet>
   <text:list-level-style-bullet text:level="2" text:bullet-char="◦"><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" fo:text-indent="-0.25in" fo:margin-left="0.75in"/></style:list-level-properties></text:list-level-style-bullet>
  </text:list-style>
  <text:list-style style:name="L2">
   <text:list-level-style-number text:level="1" style:num-format="1" style:num-suffix="."><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" fo:text-indent="-0.25in" fo:margin-left="0.5in"/></style:list-level-properties></text:list-level-style-number>
   <text:list-level-style-number text:level="2" style:num-format="a" style:num-suffix=")"><style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" fo:text-indent="-0.25in" fo:margin-left="0.75in"/></style:list-level-properties></text:list-level-style-number>
  </text:list-style>
 </office:automatic-styles>
 <office:body><office:text>
  <text:h text:style-name="Heading_20_1" text:outline-level="1">Fixture Heading One</text:h>
  <text:p>Plain text, <text:span text:style-name="T1">bold</text:span>, <text:span text:style-name="T2">italic</text:span>, <text:span text:style-name="T3">underlined</text:span>, <text:span text:style-name="T4">struck</text:span> and <text:span text:style-name="Source_20_Text">mono code</text:span>. A <text:a xlink:type="simple" xlink:href="https://example.org/page?a=1&amp;b=2">link to example</text:a> and a note<text:note text:id="ftn1" text:note-class="footnote"><text:note-citation>1</text:note-citation><text:note-body><text:p>The footnote text.</text:p></text:note-body></text:note>.</text:p>
  <text:h text:style-name="Heading_20_2" text:outline-level="2">Lists</text:h>
  <text:list text:style-name="L1">
   <text:list-item><text:p>Bullet one</text:p>
    <text:list><text:list-item><text:p>Nested bullet</text:p></text:list-item></text:list>
   </text:list-item>
   <text:list-item><text:p>Bullet two</text:p></text:list-item>
  </text:list>
  <text:list text:style-name="L2">
   <text:list-item><text:p>First step</text:p></text:list-item>
   <text:list-item><text:p>Second step</text:p>
    <text:list><text:list-item><text:p>Sub step a</text:p></text:list-item><text:list-item><text:p>Sub step b</text:p></text:list-item></text:list>
   </text:list-item>
  </text:list>
  <text:h text:style-name="Heading_20_3" text:outline-level="3">Table</text:h>
  <table:table table:name="Table1">
   <table:table-column table:number-columns-repeated="3"/>
   <table:table-row>
    <table:table-cell office:value-type="string"><text:p>Item</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Qty</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>Note</text:p></table:table-cell>
   </table:table-row>
   <table:table-row>
    <table:table-cell office:value-type="string"><text:p>Laptop</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p>2</text:p></table:table-cell>
    <table:table-cell office:value-type="string"><text:p><text:span text:style-name="T1">tested</text:span></text:p></table:table-cell>
   </table:table-row>
   <table:table-row>
    <table:table-cell table:number-columns-spanned="2" office:value-type="string"><text:p>Merged across two</text:p></table:table-cell>
    <table:covered-table-cell/>
    <table:table-cell office:value-type="string"><text:p>end</text:p></table:table-cell>
   </table:table-row>
  </table:table>
  <text:p text:style-name="Preformatted_20_Text">let x = 1;</text:p>
  <text:p text:style-name="Preformatted_20_Text">print(x)</text:p>
  <text:p text:style-name="Quotations">A quoted paragraph.</text:p>
  <text:p>Outline only paragraph follows.</text:p>
  <text:h text:outline-level="4">Level four by outline</text:h>
  <text:p>Image: <draw:frame draw:name="Red box" svg:width="2cm" svg:height="1cm" text:anchor-type="as-char"><draw:image><office:binary-data>$img64</office:binary-data></draw:image><svg:title>Red box</svg:title><svg:desc>A small red rectangle</svg:desc></draw:frame></text:p>
  <text:p text:style-name="P1">After the page break.</text:p>
 </office:text></office:body>
</office:document>
EOF

cd "$tmp"
"${LO[@]}" --convert-to xlsx basic.csv --outdir "$tmp" >/dev/null 2>&1
"${LO[@]}" --convert-to xlsx rich.fods --outdir "$tmp" >/dev/null 2>&1
"${LO[@]}" --convert-to 'docx:MS Word 2007 XML' doc.fodt --outdir "$tmp" >/dev/null 2>&1
cp basic.xlsx "$out/soffice-basic.xlsx"
cp rich.xlsx "$out/soffice-rich.xlsx"
cp doc.docx "$out/soffice-doc.docx"
cp red.png "$out/red.png"
ls -la "$out"
