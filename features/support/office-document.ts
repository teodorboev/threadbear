import path from 'node:path';
import { execFileSync } from 'node:child_process';

/** A tiny, valid Office document for the native preview spec and screenshot. */
export function sampleOffice(folder: string): string {
  const document = path.join(folder, 'Launch brief.docx');
  execFileSync('python3', ['-c', `import sys, zipfile
with zipfile.ZipFile(sys.argv[1], 'w', zipfile.ZIP_DEFLATED) as z:
 z.writestr('[Content_Types].xml', '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>')
 z.writestr('_rels/.rels', '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
 z.writestr('word/document.xml', '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Launch brief</w:t></w:r></w:p><w:p><w:r><w:t>Read this document beside the conversation.</w:t></w:r></w:p></w:body></w:document>')
`, document]);
  return document;
}
