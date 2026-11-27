//! The files a drawing references and reads to draw or measure: PDF pages
//! (with their passwords) and DWF/DGN sheets. Decoding them for the screen
//! is the scene's; reading them is here so the drawing model can ask a page
//! its size.

pub mod pdf_password;
pub mod pdf_source;
pub mod vector;

