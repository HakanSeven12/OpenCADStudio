// Persistent command defaults — last-used values survive across command runs
// within the same session.  Uses thread_local! so no synchronisation needed.

use std::cell::Cell;

thread_local! {
    static CIRCLE_RADIUS:   Cell<f64> = Cell::new(1.0);
    static ROTATE_ANGLE:    Cell<f64> = Cell::new(0.0);   // degrees
    static SCALE_FACTOR:    Cell<f64> = Cell::new(1.0);
    static OFFSET_DIST:     Cell<f64> = Cell::new(1.0);
    static FILLET_RADIUS:   Cell<f64> = Cell::new(1.0);
    static CHAMFER_DIST1:   Cell<f64> = Cell::new(10.0);
    static CHAMFER_DIST2:   Cell<f64> = Cell::new(10.0);
    static ARRAY_ROWS:      Cell<f64> = Cell::new(2.0);
    static ARRAY_COLS:      Cell<f64> = Cell::new(2.0);
    static ARRAY_ROW_SP:    Cell<f64> = Cell::new(100.0);
    static ARRAY_COL_SP:    Cell<f64> = Cell::new(100.0);
    static ARRAY_P_COUNT:   Cell<f64> = Cell::new(6.0);
    static ARRAY_P_ANGLE:   Cell<f64> = Cell::new(360.0); // degrees
    static ARRAY_PATH_COUNT: Cell<f64> = Cell::new(6.0);
    static POLYGON_SIDES:   Cell<f64> = Cell::new(6.0);
    static RECT_ELEVATION:  Cell<f64> = Cell::new(0.0);
    static RECT_THICKNESS:  Cell<f64> = Cell::new(0.0);
    static RECT_WIDTH:      Cell<f64> = Cell::new(0.0);
    static RECT_ROTATION:   Cell<f64> = Cell::new(0.0);   // degrees
    static RECT_CHAMFER1:   Cell<f64> = Cell::new(0.0);
    static RECT_CHAMFER2:   Cell<f64> = Cell::new(0.0);
    static RECT_FILLET:     Cell<f64> = Cell::new(0.0);
}

macro_rules! accessors {
    ($get:ident, $set:ident, $var:ident) => {
        pub fn $get() -> f64 {
            $var.with(|c| c.get())
        }
        pub fn $set(v: f64) {
            $var.with(|c| c.set(v));
        }
    };
}

accessors!(get_circle_radius, set_circle_radius, CIRCLE_RADIUS);
pub fn get_circle_diam() -> f64 {
    get_circle_radius() * 2.0
}

pub fn set_circle_diam(value: f64) {
    set_circle_radius(value * 0.5);
}
accessors!(get_rotate_angle, set_rotate_angle, ROTATE_ANGLE);
accessors!(get_scale_factor, set_scale_factor, SCALE_FACTOR);
accessors!(get_offset_dist, set_offset_dist, OFFSET_DIST);
accessors!(get_fillet_radius, set_fillet_radius, FILLET_RADIUS);
accessors!(get_chamfer_dist1, set_chamfer_dist1, CHAMFER_DIST1);
accessors!(get_chamfer_dist2, set_chamfer_dist2, CHAMFER_DIST2);
accessors!(get_array_rows, set_array_rows, ARRAY_ROWS);
accessors!(get_array_cols, set_array_cols, ARRAY_COLS);
accessors!(get_array_row_sp, set_array_row_sp, ARRAY_ROW_SP);
accessors!(get_array_col_sp, set_array_col_sp, ARRAY_COL_SP);
accessors!(get_array_p_count, set_array_p_count, ARRAY_P_COUNT);
accessors!(get_array_p_angle, set_array_p_angle, ARRAY_P_ANGLE);
accessors!(get_array_path_count, set_array_path_count, ARRAY_PATH_COUNT);
accessors!(get_polygon_sides, set_polygon_sides, POLYGON_SIDES);
accessors!(get_rect_elevation, set_rect_elevation, RECT_ELEVATION);
accessors!(get_rect_thickness, set_rect_thickness, RECT_THICKNESS);
accessors!(get_rect_width, set_rect_width, RECT_WIDTH);
accessors!(get_rect_rotation, set_rect_rotation, RECT_ROTATION);
accessors!(get_rect_chamfer1, set_rect_chamfer1, RECT_CHAMFER1);
accessors!(get_rect_chamfer2, set_rect_chamfer2, RECT_CHAMFER2);
accessors!(get_rect_fillet, set_rect_fillet, RECT_FILLET);
