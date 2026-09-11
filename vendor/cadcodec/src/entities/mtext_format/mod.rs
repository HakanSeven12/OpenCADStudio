//! MTEXT format string parser and serializer.
//!
//! This module provides utilities for parsing AutoCAD MTEXT formatted strings
//! into a structured representation of paragraphs and styled text spans, and
//! for serializing such structures back to MTEXT format strings.
//!
//! # MTEXT Format
//!
//! MTEXT uses an RTF-like formatting syntax where the entire formatted block
//! is enclosed in braces `{...}`. Control codes start with `\` followed by
//! a letter and take optional semicolon-separated arguments.
//!
//! # Supported Control Codes
//!
//! | Code | Description |
//! |------|-------------|
//! | `\P` | Paragraph break |
//! | `\N` | New column break |
//! | `\X` | Wrap at dimension line |
//! | `\~` | Non-breaking space |
//! | `\U+XXXX` | Unicode escape |
//! | `\L` / `\l` | Underline on / off |
//! | `\O` / `\o` | Overline on / off |
//! | `\K` / `\k` | Strikethrough on / off |
//! | `\C<n>;` | ACI color index (0 = ByBlock, 256 = ByLayer) |
//! | `\c<n>;` | True-color RGB (packed BGR) |
//! | `\H<n>;` / `\H<n>x;` | Height absolute / relative |
//! | `\W<n>;` / `\W<n>x;` | Width factor absolute / relative |
//! | `\T<n>;` / `\T<n>x;` | Tracking absolute / relative |
//! | `\Q<n>;` | Oblique angle |
//! | `\A<n>;` | Line alignment (0=bottom, 1=middle, 2=top) |
//! | `\f<family>|b0/b1|i0/i1|c<n>|p<n>|...;` | Font with bold/italic/charset/pitch flags |
//! | `\F<N><name>.shx;` | Font by SHX name |
//! | `\F<n>;` | Fraction style (0–3) |
//! | `\S<num>/<den>;` | Stacking (fractions, limits) |
//! | `\p q<l/r/c/j/d>;` | Paragraph alignment (left/right/center/justified/distributed) |
//! | `\p i<n>;` | First line indent (inches) |
//! | `\p l<n>;` | Left margin (inches) |
//! | `\p r<n>;` | Right margin (inches) |
//! | `\p b<n>;` | Spacing before paragraph (inches) |
//! | `\p a<n>;` | Spacing after paragraph (inches) |
//! | `\p se<n>;` | Exact line spacing (inches) |
//! | `\p sm<n>;` | Line spacing as multiple of font height |
//! | `\p t<n1,n2,...>;` | Tab stop positions |
//! | `\p...;` | Paragraph properties (sub-codes comma-separated) |
//! | `\b<n>;` | Legacy strikethrough (1=on, 0=off) |
//! | `{...}` | Style group (push/pop context) |
//! | `%%c` | Diameter symbol (Ø) |
//! | `%%d` | Degree symbol (°) |
//! | `%%p` | Plus-minus symbol (±) |
//! | `%%%%` | Literal percent sign (%) |
//!
//! # Escaping
//!
//! - `\\` → literal backslash
//! - `\{` → literal opening brace
//! - `\}` → literal closing brace
//!
//! # Example
//!
//! ```
//! use acadrust::entities::mtext_format::{
//!     parse_mtext, MTextDocument, MTextParagraph, MTextSpan, SpanProperties, MTextColor,
//! };
//!
//! // Parse an MTEXT format string
//! let doc = parse_mtext("{\\C1;Red text\\C0;; normal}", false);
//! assert_eq!(doc.paragraphs.len(), 1);
//! assert_eq!(doc.paragraphs[0].spans.len(), 2);
//!
//! // Access span properties
//! let red_span = &doc.paragraphs[0].spans[0];
//! assert_eq!(red_span.text, "Red text");
//! assert_eq!(red_span.properties.color, Some(MTextColor::Index(1)));
//!
//! // Get plain text
//! let plain = doc.to_plain_text();
//! assert_eq!(plain, "Red text normal");
//!
//! // Build programmatically
//! let mut doc = MTextDocument::new();
//! let mut para = MTextParagraph::new();
//!
//! let mut props = SpanProperties::default();
//! props.color = Some(MTextColor::Index(5));
//! para.push_span(MTextSpan::new("Blue", props));
//! para.push_span(MTextSpan::plain(" text"));
//!
//! doc.push_paragraph(para);
//!
//! // Serialize back to MTEXT format
//! let output = doc.to_mtext_string();
//! assert!(output.contains("\\C5;"));
//! ```

pub mod parser;
pub mod types;

pub use parser::parse_mtext;
pub use parser::parse_plain_text;
pub use types::{
    MTextColor, MTextDocument, MTextFont, MTextLineAlignment, MTextLineSpacing, MTextParagraph,
    MTextParagraphAlignment, MTextScalar, MTextSpan, MTextStroke, ParagraphProperties,
    SpanProperties, SpecialChar, StackingData, StackingType, TabStop,
};
