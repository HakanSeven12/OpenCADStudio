use crate::t;
use codec::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "BLOCK",
        label: "Create Block",
        icon: IconKind::Svg(include_bytes!("../../../assets/icons/blocks/block.svg")),
        event: ModuleEvent::Command("BLOCK".to_string()),
    }
}

enum Step {
    Name,
    Base { name: String },
}

pub struct CreateBlockCommand {
    handles: Vec<Handle>,
    step: Step,
}

impl CreateBlockCommand {
    pub fn new(handles: Vec<Handle>) -> Self {
        Self {
            handles,
            step: Step::Name,
        }
    }
}

impl CadCommand for CreateBlockCommand {
    fn name(&self) -> &'static str {
        "BLOCK"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Name => t!(
                "BLOCK  Enter block name  [%{count} objects selected]:",
                count = self.handles.len()
            )
            .into_owned(),
            Step::Base { name } => t!(
                "BLOCK  Specify base point for \"%{name}\"  [%{count} objects]:",
                name = name,
                count = self.handles.len()
            )
            .into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            Step::Name => CmdResult::NeedPoint,
            Step::Base { name } => CmdResult::CreateBlock {
                handles: self.handles.clone(),
                name: name.clone(),
                base: pt,
            },
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, Step::Name)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if !matches!(self.step, Step::Name) {
            return None;
        }
        let name = text.trim();
        if name.is_empty() {
            return None;
        }
        self.step = Step::Base {
            name: name.to_string(),
        };
        Some(CmdResult::NeedPoint)
    }

    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }
}

/// One-shot point picker for the Block Definition dialog's "Pick point" button.
pub struct BlockPickBasePointCommand;

impl CadCommand for BlockPickBasePointCommand {
    fn name(&self) -> &'static str {
        "BLOCK"
    }

    fn prompt(&self) -> String {
        crate::t!("BLOCK  Specify insertion base point:").into_owned()
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::Dispatch(format!("BLOCK_POINT_PICKED {} {} {}", pt.x, pt.y, pt.z))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch("BLOCK_POINT_CANCELLED".to_string())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch("BLOCK_POINT_CANCELLED".to_string())
    }
}

/// Command handling the on-screen prompting sequence when "Specify On-screen" is checked in the dialog.
/// Enforces standard command ordering: Base point is ALWAYS prompted before object selection.
pub struct BlockOnScreenCommand {
    options: crate::scene::CreateBlockOptions,
    ucs: crate::app::helpers::UcsXform,
    need_base: bool,
    need_objects: bool,
}

impl BlockOnScreenCommand {
    pub fn new(
        options: crate::scene::CreateBlockOptions,
        ucs: crate::app::helpers::UcsXform,
        need_base: bool,
        need_objects: bool,
    ) -> Self {
        Self {
            options,
            ucs,
            need_base,
            need_objects,
        }
    }
}

impl CadCommand for BlockOnScreenCommand {
    fn name(&self) -> &'static str {
        "BLOCK"
    }

    fn prompt(&self) -> String {
        if self.need_base {
            crate::t!("BLOCK  Specify insertion base point:").into_owned()
        } else {
            crate::t!("BLOCK  Select objects:").into_owned()
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.need_base {
            self.options.base_point = pt;
            self.options.world_to_block = self.ucs.to_ucs_transform_at(pt);
            self.options.block_to_world = self.ucs.to_wcs_transform_at(pt);
            self.need_base = false;
            if self.need_objects {
                CmdResult::NeedPoint
            } else {
                CmdResult::CreateBlockWithOptions {
                    options: Box::new(self.options.clone()),
                }
            }
        } else {
            CmdResult::NeedPoint
        }
    }

    fn is_selection_gathering(&self) -> bool {
        !self.need_base && self.need_objects
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.options.handles = handles;
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.need_base {
            return CmdResult::Cancel;
        }
        if self.options.handles.is_empty() {
            return CmdResult::Cancel;
        }
        CmdResult::CreateBlockWithOptions {
            options: Box::new(self.options.clone()),
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{CreateBlockOptions, Scene};
    use crate::ui::window::block_definition::BlockObjectMode;
    use codec::entities::Line;
    use codec::types::{Transform, Vector3};

    fn make_test_scene() -> (Scene, Handle) {
        let mut scene = Scene::new();
        let mut line = Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 10.0, 0.0);
        let h = scene.add_entity(codec::EntityType::Line(line));
        (scene, h)
    }

    #[test]
    fn test_create_block_retain_mode() {
        let (mut scene, line_h) = make_test_scene();
        let options = CreateBlockOptions {
            name: "BLK_RETAIN".to_string(),
            handles: vec![line_h],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Retain,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: 0,
            description: "Retain test".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };

        let result = scene.create_block_with_options(options);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Handle::NULL); // No insert created in Retain mode

        // Original line must still exist in document
        assert!(scene.document.get_entity(line_h).is_some());
        // Block definition must exist
        assert!(scene.document.block_records.get("BLK_RETAIN").is_some());
    }

    #[test]
    fn test_create_block_convert_mode() {
        let (mut scene, line_h) = make_test_scene();
        let options = CreateBlockOptions {
            name: "BLK_CONVERT".to_string(),
            handles: vec![line_h],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Convert,
            annotative: false,
            match_orientation: false,
            scale_uniformly: true,
            allow_exploding: true,
            unit: 4, // mm
            description: "Convert test".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };

        let result = scene.create_block_with_options(options);
        assert!(result.is_ok());
        let insert_h = result.unwrap();
        assert_ne!(insert_h, Handle::NULL);

        // Original line was erased from model space
        assert!(scene.document.get_entity(line_h).is_none());
        // Insert entity exists in drawing
        let insert_entity = scene.document.get_entity(insert_h);
        assert!(insert_entity.is_some());
        if let Some(codec::EntityType::Insert(ins)) = insert_entity {
            assert_eq!(ins.block_name, "BLK_CONVERT");
        } else {
            panic!("Expected Insert entity");
        }

        // Check BlockRecord properties
        let br = scene.document.block_records.get("BLK_CONVERT").unwrap();
        assert_eq!(br.units, 4);
        assert!(br.scale_uniformly);
        assert!(br.explodable);
        assert_eq!(br.description, "Convert test");
    }

    #[test]
    fn test_create_block_delete_mode() {
        let (mut scene, line_h) = make_test_scene();
        let options = CreateBlockOptions {
            name: "BLK_DELETE".to_string(),
            handles: vec![line_h],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Delete,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: 0,
            description: "".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };

        let result = scene.create_block_with_options(options);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Handle::NULL);

        // Original line erased
        assert!(scene.document.get_entity(line_h).is_none());
        // Block definition exists
        assert!(scene.document.block_records.get("BLK_DELETE").is_some());
    }

    #[test]
    fn test_create_block_redefinition() {
        let (mut scene, line1_h) = make_test_scene();
        let options1 = CreateBlockOptions {
            name: "REDEF_BLOCK".to_string(),
            handles: vec![line1_h],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Retain,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: 0,
            description: "Initial".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };

        assert!(scene.create_block_with_options(options1).is_ok());

        // Attempting to create duplicate without redefine flag fails
        let line2_h = {
            let mut line2 = Line::new();
            line2.start = Vector3::new(5.0, 5.0, 0.0);
            line2.end = Vector3::new(15.0, 15.0, 0.0);
            scene.add_entity(codec::EntityType::Line(line2))
        };

        let options2 = CreateBlockOptions {
            name: "REDEF_BLOCK".to_string(),
            handles: vec![line2_h],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Retain,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: 6, // meters
            description: "Redefined".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };

        let err_res = scene.create_block_with_options(options2.clone());
        assert!(err_res.is_err());
        assert!(err_res.unwrap_err().contains("already exists"));

        // Redefine with redefine: true succeeds and updates fields
        let mut options3 = options2;
        options3.redefine = true;
        let redef_res = scene.create_block_with_options(options3);
        assert!(redef_res.is_ok());

        let br = scene.document.block_records.get("REDEF_BLOCK").unwrap();
        assert_eq!(br.description, "Redefined");
        assert_eq!(br.units, 6);
    }

    #[test]
    fn test_create_block_validation() {
        let (mut scene, line_h) = make_test_scene();

        // Empty name
        let opt_empty_name = CreateBlockOptions {
            name: "   ".to_string(),
            handles: vec![line_h],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Retain,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: 0,
            description: "".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };
        assert!(scene.create_block_with_options(opt_empty_name).is_err());

        // Anonymous '*' prefix blocked
        let opt_star = CreateBlockOptions {
            name: "*ANON".to_string(),
            handles: vec![line_h],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Retain,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: 0,
            description: "".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };
        assert!(scene.create_block_with_options(opt_star).is_err());

        // Empty handles
        let opt_no_handles = CreateBlockOptions {
            name: "EMPTY_HANDLES".to_string(),
            handles: vec![],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Retain,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: 0,
            description: "".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };
        assert!(scene.create_block_with_options(opt_no_handles).is_err());
    }

    #[test]
    fn test_block_on_screen_command_ordering() {
        let dummy_options = CreateBlockOptions {
            name: "ON_SCREEN_TEST".to_string(),
            handles: vec![],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Convert,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: 0,
            description: "".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };

        // When both base point and objects are to be specified on-screen:
        let ucs = crate::app::helpers::UcsXform::identity();
        let mut cmd = BlockOnScreenCommand::new(dummy_options, ucs, true, true);

        // 1. Initial step must be insertion base point
        assert!(cmd.prompt().contains("Specify insertion base point"));

        // 2. Pick base point -> prompt switches to object selection
        let picked_pt = DVec3::new(10.0, 20.0, 0.0);
        let res1 = cmd.on_point(picked_pt);
        assert!(matches!(res1, CmdResult::NeedPoint));
        assert!(cmd.prompt().contains("Select objects"));
        assert!(cmd.is_selection_gathering());

        // 3. User attempts enter before selecting objects -> cancelled
        assert!(matches!(cmd.on_enter(), CmdResult::Cancel));

        // 4. Complete selection with handles
        let fake_handle = Handle::new(42);
        cmd.on_selection_complete(vec![fake_handle]);

        // 5. User presses enter to finish selection
        let finish = cmd.on_enter();
        match finish {
            CmdResult::CreateBlockWithOptions { options } => {
                assert_eq!(options.name, "ON_SCREEN_TEST");
                assert_eq!(options.base_point, picked_pt);
                assert_eq!(options.handles, vec![fake_handle]);
                assert_eq!(options.world_to_block, ucs.to_ucs_transform_at(picked_pt));
                assert_eq!(options.block_to_world, ucs.to_wcs_transform_at(picked_pt));
            }
            _ => panic!("Expected CreateBlockWithOptions"),
        }
    }

    #[test]
    fn test_block_on_screen_base_point_sets_correct_transforms() {
        let (mut scene, line_h) = make_test_scene();
        let initial_options = CreateBlockOptions {
            name: "ON_SCREEN_BASE_TEST".to_string(),
            handles: vec![line_h],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Convert,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: 0,
            description: "".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };

        // User picked objects upfront in dialog, but checked "Specify base point on screen"
        let ucs = crate::app::helpers::UcsXform::identity();
        let mut cmd = BlockOnScreenCommand::new(initial_options, ucs, true, false);

        // Pick base point at (10, 10, 0) (end of the line in make_test_scene)
        let pick_pt = DVec3::new(10.0, 10.0, 0.0);
        let res = cmd.on_point(pick_pt);

        let created_options = match res {
            CmdResult::CreateBlockWithOptions { options } => *options,
            _ => {
                panic!("Expected CreateBlockWithOptions immediately when objects already selected")
            }
        };

        assert_eq!(created_options.base_point, pick_pt);
        assert_eq!(
            created_options.world_to_block,
            ucs.to_ucs_transform_at(pick_pt)
        );
        assert_eq!(
            created_options.block_to_world,
            ucs.to_wcs_transform_at(pick_pt)
        );

        // Create the block with these options and verify insert position and definition entities
        let insert_h = scene.create_block_with_options(created_options).unwrap();
        assert_ne!(insert_h, Handle::NULL);

        // In Convert mode, the insert entity must be placed at the picked base point
        let insert_entity = scene.document.get_entity(insert_h).unwrap();
        if let codec::EntityType::Insert(ins) = insert_entity {
            assert_eq!(ins.block_name, "ON_SCREEN_BASE_TEST");
            assert!((ins.insert_point.x - 10.0).abs() < 1e-6);
            assert!((ins.insert_point.y - 10.0).abs() < 1e-6);
            assert!((ins.insert_point.z - 0.0).abs() < 1e-6);
        } else {
            panic!("Expected Insert entity");
        }

        // Inside the block table record, the line entity must have been transformed relative to (10, 10, 0)
        // Original line was (0,0,0) to (10,10,0), so inside the block it must be (-10,-10,0) to (0,0,0)
        let br = scene
            .document
            .block_records
            .get("ON_SCREEN_BASE_TEST")
            .unwrap();
        let block_line = scene
            .document
            .entities()
            .find(|e| {
                e.common().owner_handle == br.handle && matches!(e, codec::EntityType::Line(_))
            })
            .unwrap();
        if let codec::EntityType::Line(l) = block_line {
            assert!((l.start.x - (-10.0)).abs() < 1e-6);
            assert!((l.start.y - (-10.0)).abs() < 1e-6);
            assert!((l.end.x - 0.0).abs() < 1e-6);
            assert!((l.end.y - 0.0).abs() < 1e-6);
        } else {
            panic!("Expected Line entity inside block");
        }
    }

    #[test]
    fn test_explodable_flag_prevents_explode() {
        let (mut scene, line_h) = make_test_scene();
        let options = CreateBlockOptions {
            name: "BLK_NO_EXPLODE".to_string(),
            handles: vec![line_h],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Convert,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: false,
            unit: 0,
            description: "".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };
        let insert_h = scene.create_block_with_options(options).unwrap();
        let insert = scene.document.get_entity(insert_h).unwrap();
        let pieces = crate::modules::draw::modify::explode::explode_entity(insert, &scene.document);
        assert!(pieces.is_empty(), "Unexplodable block must not explode");
    }

    #[test]
    fn test_explodable_flag_allows_explode() {
        let (mut scene, line_h) = make_test_scene();
        let options = CreateBlockOptions {
            name: "BLK_CAN_EXPLODE".to_string(),
            handles: vec![line_h],
            base_point: DVec3::ZERO,
            world_to_block: Transform::identity(),
            block_to_world: Transform::identity(),
            mode: BlockObjectMode::Convert,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: 0,
            description: "".to_string(),
            hyperlink_url: "".to_string(),
            hyperlink_desc: "".to_string(),
            redefine: false,
        };
        let insert_h = scene.create_block_with_options(options).unwrap();
        let insert = scene.document.get_entity(insert_h).unwrap();
        let pieces = crate::modules::draw::modify::explode::explode_entity(insert, &scene.document);
        assert_eq!(
            pieces.len(),
            1,
            "Explodable block should explode into 1 line"
        );
    }
}
