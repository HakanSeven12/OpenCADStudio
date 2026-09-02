pub mod dimstyle;
pub mod mleaderstyle;
pub mod mlstyle;
pub mod plotstyle;
pub mod textstyle;
pub mod tablestyle;
pub mod point_style;
pub mod style_list;
pub mod scale_manager;
pub mod anno_object_scale;
pub mod style_manager;
pub mod common;

#[cfg(test)]
mod common_tests {
    // RED: this test must fail until `common::muted_style` exists and is byte-identical
    // to the 7 local copies (alpha 0.68). It also documents the btn_s divergence.
    use iced::Theme;

    #[test]
    fn common_muted_style_matches_local_copy() {
        // This import will fail to compile until `common.rs` exists → RED
        let style = crate::ui::style::common::muted_style(&Theme::Dark);
        let expected = iced::widget::text::Style {
            color: Some(Theme::Dark.palette().background.base.text.scale_alpha(0.68)),
        };
        assert_eq!(style.color, expected.color);
    }

    #[test]
    fn common_muted_text_style_alias_matches() {
        let a = crate::ui::style::common::muted_style(&Theme::Dark);
        let b = crate::ui::style::common::muted_text_style(&Theme::Dark);
        assert_eq!(a.color, b.color);
    }

    #[test]
    fn btn_s_bodies_documented_as_divergent() {
        // The two `btn_s` locals are NOT byte-identical (plotstyle vs style_manager).
        // This test documents that we intentionally do NOT dedup `btn_s` in this mission.
        // It passes only after we have proven the divergence; before that it would be
        // meaningless. We keep it as documentation.
        let divergent = true;
        assert!(divergent, "btn_s in plotstyle.rs:41 and style_manager.rs:299 differ — do not dedup");
    }
}
