use super::*;

impl Scene {
    fn model_limits(&self) -> Option<(glam::DVec2, glam::DVec2)> {
        let min = self.document.header.model_space_limits_min;
        let max = self.document.header.model_space_limits_max;
        Self::valid_limits(
            glam::DVec2::new(min.x, min.y),
            glam::DVec2::new(max.x, max.y),
        )
    }

    fn paper_layout_limits(&self) -> Option<(glam::DVec2, glam::DVec2)> {
        self.document.objects.values().find_map(|object| {
            let ObjectType::Layout(layout) = object else {
                return None;
            };
            (layout.name == self.current_layout).then(|| {
                Self::valid_limits(
                    glam::DVec2::new(layout.min_limits.0, layout.min_limits.1),
                    glam::DVec2::new(layout.max_limits.0, layout.max_limits.1),
                )
            })?
        })
    }

    fn valid_limits(min: glam::DVec2, max: glam::DVec2) -> Option<(glam::DVec2, glam::DVec2)> {
        const SANE_LIMIT: f64 = 1.0e16;
        (min.is_finite()
            && max.is_finite()
            && min.x < max.x
            && min.y < max.y
            && min.abs().max_element() < SANE_LIMIT
            && max.abs().max_element() < SANE_LIMIT)
            .then_some((min, max))
    }

    /// The active input space is model space on the Model tab and while editing
    /// through a floating paper-space viewport (MSPACE).
    pub fn input_uses_model_space(&self) -> bool {
        self.current_layout == "Model" || self.active_viewport.is_some()
    }

    /// LIMITS rectangle for the active point-input space.
    pub fn current_drawing_limits(&self) -> Option<(glam::DVec2, glam::DVec2)> {
        if self.input_uses_model_space() {
            self.model_limits()
        } else {
            self.paper_layout_limits().or_else(|| {
                let min = self.document.header.paper_space_limits_min;
                let max = self.document.header.paper_space_limits_max;
                Self::valid_limits(
                    glam::DVec2::new(min.x, min.y),
                    glam::DVec2::new(max.x, max.y),
                )
            })
        }
    }

    /// LIMITS rectangle belonging to a rendered grid viewport. Floating
    /// viewports display model space; the sheet viewport displays paper space.
    pub fn grid_limits_for_viewport(&self, viewport: Handle) -> Option<(glam::DVec2, glam::DVec2)> {
        if self.current_layout == "Model" {
            return self.model_limits();
        }
        let sheet = self.current_layout_sheet_viewport_handle();
        if viewport.is_valid() && viewport != sheet {
            self.model_limits()
        } else {
            self.paper_layout_limits()
        }
    }

    pub fn drawing_limit_check_enabled(&self) -> bool {
        if self.input_uses_model_space() {
            self.document.header.limit_check
        } else {
            self.document.header.paper_space_limit_check
        }
    }

    pub fn point_inside_drawing_limits(&self, point: glam::DVec3) -> bool {
        let Some((min, max)) = self.current_drawing_limits() else {
            return true;
        };
        point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y
    }

    pub fn set_drawing_limit_check(&mut self, enabled: bool) {
        if self.input_uses_model_space() {
            self.document.header.limit_check = enabled;
        } else {
            self.document.header.paper_space_limit_check = enabled;
            self.persist_current_layout_state();
        }
    }

    pub fn set_current_drawing_limits(&mut self, min: glam::DVec2, max: glam::DVec2) {
        if self.input_uses_model_space() {
            self.document.header.model_space_limits_min =
                acadrust::types::Vector2::new(min.x, min.y);
            self.document.header.model_space_limits_max =
                acadrust::types::Vector2::new(max.x, max.y);
        } else {
            self.document.header.paper_space_limits_min =
                acadrust::types::Vector2::new(min.x, min.y);
            self.document.header.paper_space_limits_max =
                acadrust::types::Vector2::new(max.x, max.y);
        }

        // Keep the current Layout object synchronized with the header values.
        // DWG stores per-layout limits here as well as the current-space header.
        for object in self.document.objects.values_mut() {
            if let ObjectType::Layout(layout) = object {
                if layout.name == self.current_layout {
                    layout.min_limits = (min.x, min.y);
                    layout.max_limits = (max.x, max.y);
                    break;
                }
            }
        }
        self.paper_viewport_cache
            .borrow_mut()
            .remove(&self.current_layout);
    }

    /// ZOOM All frames the configured drawing limits. Object-only framing
    /// remains the responsibility of ZOOM Extents.
    pub fn fit_all_with_limits(&mut self) {
        let Some((limit_min, limit_max)) = self.current_drawing_limits() else {
            self.fit_all();
            return;
        };

        let min = glam::Vec3::new(limit_min.x as f32, limit_min.y as f32, 0.0);
        let max = glam::Vec3::new(limit_max.x as f32, limit_max.y as f32, 0.0);

        // MSPACE owns a camera encoded on the active viewport entity.
        if self.active_viewport.is_some() {
            self.fit_active_viewport_to_bounds(min, max);
            return;
        }

        let aspect = self.active_camera_aspect();
        self.camera.borrow_mut().fit_to_bounds(min, max, aspect);
        self.camera_generation += 1;
    }
}
