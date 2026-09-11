// HATCHEDIT — edit an existing hatch entity's pattern, scale, or angle.
//
// Workflow:
//   1. Pick or pre-select a Hatch entity.
//   2. Enter options:
//        P <name>     — change pattern (ANSI31, SOLID, etc.)
//        S <value>    — change scale
//        A <degrees>  — change angle
//      Press Enter to apply changes.

use acadrust::Handle;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, HatchEditOperation};

enum HatcheditStep {
    PickHatch,
    EditOptions {
        handle: Handle,
        name: String,
        scale: f32,
        angle: f32,
    },
}

pub struct HatcheditCommand {
    step: HatcheditStep,
    origin: Option<(f64, f64)>,
    disassociate: bool,
    style: Option<acadrust::entities::HatchStyleType>,
    annotative: Option<bool>,
    annotative_current: bool,
    style_current: acadrust::entities::HatchStyleType,
    input: Option<&'static str>,
    boundary_selection: Vec<Handle>,
    source_appearance: Option<(acadrust::types::Color,String,acadrust::types::Transparency)>,
    current_color: acadrust::types::Color,
    current_transparency: acadrust::types::Transparency,
    boundary_region: bool,
    association_sources: Option<(crate::command::WorkingPlane,rustc_hash::FxHashMap<Handle,crate::scene::BoundarySource>)>,
    association_paths: Vec<acadrust::entities::BoundaryPath>,
    association_missed: bool,
}

impl HatcheditCommand {
    pub fn new() -> Self {
        Self {
            step: HatcheditStep::PickHatch,
            origin: None,
            disassociate: false,
            style: None,
            annotative: None,
            annotative_current: false,
            style_current: acadrust::entities::HatchStyleType::Normal,
            input: None,
            boundary_selection: Vec::new(),
            source_appearance: None,
            current_color: acadrust::types::Color::ByLayer,
            current_transparency: acadrust::types::Transparency::ByLayer,
            boundary_region: false,
            association_sources: None,
            association_paths: Vec::new(),
            association_missed: false,
        }
    }

    pub fn with_handle(
        handle: Handle,
        name: String,
        scale: f32,
        angle: f32,
        annotative: bool,
    ) -> Self {
        Self {
            step: HatcheditStep::EditOptions {
                handle,
                name,
                scale,
                angle,
            },
            origin: None,
            disassociate: false,
            style: None,
            annotative: None,
            annotative_current: annotative,
            style_current: acadrust::entities::HatchStyleType::Normal,
            input: None,
            boundary_selection: Vec::new(),
            source_appearance: None,
            current_color: acadrust::types::Color::ByLayer,
            current_transparency: acadrust::types::Transparency::ByLayer,
            boundary_region: false,
            association_sources: None,
            association_paths: Vec::new(),
            association_missed: false,
        }
    }

    fn apply_result(&self, operation: HatchEditOperation) -> Option<CmdResult> {
        let HatcheditStep::EditOptions {
            handle,
            name,
            scale,
            angle,
        } = &self.step
        else {
            return None;
        };
        Some(CmdResult::HatcheditApply {
            handle: *handle,
            name: name.clone(),
            scale: *scale,
            angle: *angle,
            operation,
        })
    }

    pub fn for_association(handle:Handle,name:String,scale:f32,angle:f32,plane:crate::command::WorkingPlane,sources:rustc_hash::FxHashMap<Handle,crate::scene::BoundarySource>)->Self {
        let mut command=Self::with_handle(handle,name,scale,angle,false);
        command.input=Some("associate-point");command.association_sources=Some((plane,sources));command
    }
    fn add_association_rings(&mut self,rings:Vec<Vec<[f64;2]>>,sources:&rustc_hash::FxHashMap<Handle,crate::scene::BoundarySource>) {
        let exterior=cadkernel::geom2d::ring_nesting_depths(&rings).into_iter().map(|depth|depth==0).collect::<Vec<_>>();
        let paths=crate::scene::exact_hatch_paths(&rings,&exterior,sources,1e-6);
        self.association_missed=paths.is_empty()||paths.len()!=rings.len()||paths.iter().any(|path|path.boundary_handles.is_empty());
        if !self.association_missed {
            for path in paths {if !self.association_paths.contains(&path){self.association_paths.push(path);}}
        }
    }
    pub fn with_appearance(mut self,entity:Option<&acadrust::EntityType>,current_color:acadrust::types::Color,current_transparency:acadrust::types::Transparency)->Self {
        if let Some(acadrust::EntityType::Hatch(hatch)) = entity { self.style_current = hatch.style; }
        self.source_appearance=entity.map(|e|{let c=e.common();(c.color,c.layer.clone(),c.transparency)});
        self.current_color=current_color;self.current_transparency=current_transparency;self
    }

    fn update_operation(&self) -> HatchEditOperation {
        HatchEditOperation::Update {
            origin: self.origin,
            disassociate: self.disassociate,
            style: self.style,
            annotative: self.annotative,
        }
    }
}

impl CadCommand for HatcheditCommand {
    fn name(&self) -> &'static str {
        "HATCHEDIT"
    }

    fn prompt(&self) -> String {
        if let Some(input)=self.input {
            if input == "style" {
                let current = match self.style_current {
                    acadrust::entities::HatchStyleType::Normal => "Normal",
                    acadrust::entities::HatchStyleType::Outer => "Outer",
                    acadrust::entities::HatchStyleType::Ignore => "Ignore",
                };
                return format!("Enter hatching style [Ignore/Outer/Normal] <{current}>:");
            }
            if let Some((color,layer,transparency))=&self.source_appearance {
                match input {
                    "color"=>return format!("New color [Truecolor/. (for use current)] <{color:?}>:"),
                    "layer"=>return format!("Specify layer or [. (for use current)] <{layer}>:"),
                    "transparency"=>return format!("Specify transparency (0-90) or ByLayer/ByBlock <{}>:",match transparency {
                        acadrust::types::Transparency::ByLayer=>"ByLayer".into(),
                        acadrust::types::Transparency::ByBlock=>"ByBlock".into(),
                        value=>format!("{:.0}",value.as_percent()*100.0),
                    }),
                    _=>{},
                }
            }
            if let HatcheditStep::EditOptions{name,scale,angle,..}=&self.step {
                match input {
                    "pattern"=>return format!("Enter a pattern name or [Solid] <{name}>:"),
                    "scale"=>return format!("Specify a scale for the pattern <{scale:.4}>:"),
                    "angle"=>return format!("Specify an angle for the pattern <{angle:.4}>:"),
                    _=>{},
                }
            }
            return match input {
                "annotative"=>if self.annotative_current {"Make hatch annotative [Yes/No] <Y>:"} else {"Make hatch annotative [Yes/No] <N>:"},
                "pattern"=>"Enter a pattern name or [Solid]:",
                "scale"=>"Specify a scale for the pattern:",
                "angle"=>"Specify an angle for the pattern:",
                "color"=>"New color [Truecolor] <ByLayer>:",
                "truecolor"=>"Specify RGB color (red,green,blue):",
                "layer"=>"Specify layer or [. (for use current)]:",
                "transparency"=>"Specify transparency value (0-90) or ByLayer/ByBlock:",
                "draworder"=>"Enter draw order [do Not change/send to Back/bring to Front/send beHind boundary/bring in front of bounDary] <do Not change>:",
                "boundary-type"=>"Enter type of boundary object [Region/Polyline] <Polyline>:",
                "boundary-associate"=>"Reassociate hatch with new boundary? [Yes/No] <No>:",
                "associate-select"=>"Select boundary objects:",
                "associate-point" if self.association_missed=>"No closed boundary found. Specify internal point or [Select objects]:",
                "associate-point"=>"Specify internal point or [Select objects]:",
                _=>"Specify value:",
            }.into();
        }
        match &self.step {
            HatcheditStep::PickHatch => t!("HATCHEDIT  Select hatch:").into_owned(),
            HatcheditStep::EditOptions {
                name, scale, angle, ..
            } => {
                let scale = format!("{scale:.4}");
                let angle = format!("{angle:.1}");
                t!(
                    "HATCHEDIT  Pattern:%{name}  Scale:%{scale}  Angle:%{angle}  [Style/Properties/COlor/LAyer/Transparency/DRaw order/ASsociate/DIsassociate/ANnotative/recreate Boundary/separate Hatches] <Properties>:",
                    name = name,
                    scale = scale,
                    angle = angle
                )
                .into_owned()
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, HatcheditStep::PickHatch)
    }
    fn is_selection_gathering(&self)->bool {self.input==Some("associate-select")}
    fn on_selection_complete(&mut self,handles:Vec<Handle>)->CmdResult {
        self.boundary_selection=handles.clone();
        if let Some((_,sources))=&self.association_sources {
            let sources=sources.iter().filter(|(handle,_)|handles.contains(handle)).map(|(handle,source)|(*handle,source.clone())).collect();
            let rings=crate::scene::boundary_faces(&sources,1e-6);
            self.add_association_rings(rings,&sources);
        }
        self.input=Some("associate-point");CmdResult::NeedPoint
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        // Actual hatch model retrieval happens in commands.rs dispatch.
        // Store handle; name/scale/angle filled in by dispatch.
        self.step = HatcheditStep::EditOptions {
            handle,
            name: String::new(),
            scale: 1.0,
            angle: 0.0,
        };
        CmdResult::NeedPoint
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, HatcheditStep::EditOptions { .. })&&!self.is_selection_gathering()
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        if self.input==Some("associate-point") {return vec![crate::command::CmdOption::new("Select objects","S")];}
        if let Some(input) = self.input {
            use crate::command::CmdOption;
            return match input {
                "style" => vec![CmdOption::new("Ignore", "I"), CmdOption::new("Outer", "O"), CmdOption::new("Normal", "N")],
                "draworder" => vec![CmdOption::new("Do not change", "N"), CmdOption::new("Send to back", "B"), CmdOption::new("Bring to front", "F"), CmdOption::new("Behind boundary", "H"), CmdOption::new("In front of boundary", "D")],
                "annotative" | "boundary-associate" => vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
                "boundary-type" => vec![CmdOption::new("Region", "R"), CmdOption::new("Polyline", "P")],
                "color" => vec![CmdOption::new("Truecolor", "T")],
                "pattern" => vec![CmdOption::new("Solid", "SOLID")],
                _ => Vec::new(),
            };
        }
        if !matches!(self.step, HatcheditStep::EditOptions { .. }) {
            return Vec::new();
        }
        vec![
            crate::command::CmdOption::new("Style", "S"),
            crate::command::CmdOption::new("Properties", "P"),
            crate::command::CmdOption::new("Color", "CO"),
            crate::command::CmdOption::new("Layer", "LA"),
            crate::command::CmdOption::new("Transparency", "T"),
            crate::command::CmdOption::new("Draw order", "DR"),
            crate::command::CmdOption::new("Associate", "AS"),
            crate::command::CmdOption::new("Disassociate", "DI"),
            crate::command::CmdOption::new("Annotative", "AN"),
            crate::command::CmdOption::new("Recreate boundary", "B"),
            crate::command::CmdOption::new("Separate hatches", "H"),
            crate::command::CmdOption::enter("Properties"),
        ]
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword=text.trim().to_ascii_uppercase();
        if let Some(input)=self.input {
            use acadrust::types::{Color,Transparency};
            let appearance=|color,layer,transparency|HatchEditOperation::Appearance{color,layer,transparency};
            match input {
                "style" => {
                    self.style = match keyword.as_str() {
                        "I" | "IGNORE" => Some(acadrust::entities::HatchStyleType::Ignore),
                        "O" | "OUTER" => Some(acadrust::entities::HatchStyleType::Outer),
                        "N" | "NORMAL" => Some(acadrust::entities::HatchStyleType::Normal),
                        _ => return Some(CmdResult::NeedPoint),
                    };
                    return self.apply_result(self.update_operation());
                }
                "annotative" => {
                    let value = match keyword.as_str() { "Y" | "YES" => true, "N" | "NO" => false, _ => return Some(CmdResult::NeedPoint) };
                    self.annotative = Some(value);
                    return self.apply_result(self.update_operation());
                }
                "associate-point"=>{
                    if matches!(keyword.as_str(),"S"|"SELECT"|"SELECT OBJECTS") {self.input=Some("associate-select");}
                    return Some(CmdResult::NeedPoint);
                },
                "color"=>{
                    if matches!(keyword.as_str(),"T"|"TRUECOLOR") {self.input=Some("truecolor");return Some(CmdResult::NeedPoint);}
                    let color=match keyword.as_str(){"."=>Some(self.current_color),"BYLAYER"=>Some(Color::ByLayer),"BYBLOCK"=>Some(Color::ByBlock),
                        "RED"=>Some(Color::Index(1)),"YELLOW"=>Some(Color::Index(2)),"GREEN"=>Some(Color::Index(3)),
                        "CYAN"=>Some(Color::Index(4)),"BLUE"=>Some(Color::Index(5)),"MAGENTA"=>Some(Color::Index(6)),"WHITE"=>Some(Color::Index(7)),
                        n=>n.parse::<i16>().ok().filter(|v|(0..=256).contains(v)).map(Color::from_index)};
                    return Some(color.and_then(|v|self.apply_result(appearance(Some(v),None,None))).unwrap_or(CmdResult::NeedPoint));
                }
                "truecolor"=>{
                    let rgb:Option<Vec<u8>>=keyword.split(',').map(|s|s.trim().parse().ok()).collect();
                    if let Some(rgb)=rgb.filter(|rgb|rgb.len()==3) {return self.apply_result(appearance(Some(Color::from_rgb(rgb[0],rgb[1],rgb[2])),None,None));}
                }
                "layer"=>if !text.trim().is_empty(){return self.apply_result(appearance(None,Some(text.trim().to_owned()),None));},
                "transparency"=>{
                    let value=match keyword.as_str(){"."=>Some(self.current_transparency),"BYLAYER"=>Some(Transparency::BY_LAYER),"BYBLOCK"=>Some(Transparency::BY_BLOCK),
                        n=>n.parse::<u8>().ok().filter(|v|*v<=90).map(|v|Transparency::from_percent(v as f64 / 100.0))};
                    if let Some(value)=value{return self.apply_result(appearance(None,None,Some(value)));}
                }
                "draworder"=>return match keyword.as_str(){"F"|"FRONT"=>self.apply_result(HatchEditOperation::DrawOrderFront),
                    "H"|"BEHIND"=>self.apply_result(HatchEditOperation::DrawOrderBoundary{above:false}),
                    "D"=>self.apply_result(HatchEditOperation::DrawOrderBoundary{above:true}),
                    "B"|"BACK"=>self.apply_result(HatchEditOperation::DrawOrderBack),"N"|"NOT"=>Some(CmdResult::Cancel),_=>Some(CmdResult::NeedPoint)},
                "boundary-type"=>match keyword.as_str(){
                    "P"|"POLYLINE"=>{self.boundary_region=false;self.input=Some("boundary-associate");},
                    "R"|"REGION"=>{self.boundary_region=true;self.input=Some("boundary-associate");},
                    _=>{},
                },
                "boundary-associate"=>return match keyword.as_str(){
                    "Y"|"YES"=>self.apply_result(HatchEditOperation::RecreateBoundary{associate:true,region:self.boundary_region}),
                    "N"|"NO"=>self.apply_result(HatchEditOperation::RecreateBoundary{associate:false,region:self.boundary_region}),
                    _=>Some(CmdResult::NeedPoint),
                },
                "pattern"=>{
                    if crate::scene::model::hatch_patterns::find(&keyword).is_some() {
                        if let HatcheditStep::EditOptions{name,..}=&mut self.step{*name=keyword.clone();}
                        if keyword=="SOLID" {return self.apply_result(self.update_operation());}
                        self.input=Some("scale");
                    }
                }
                "scale"|"angle"=>if let Ok(value)=keyword.parse::<f32>() {if value.is_finite()&&(input=="angle"||value>0.0){
                    if let HatcheditStep::EditOptions{scale,angle,..}=&mut self.step{if input=="scale"{*scale=value;}else{*angle=value;}}
                    if input=="scale"{self.input=Some("angle");}else{return self.apply_result(self.update_operation());}
                }},
                _=>{},
            }
            return Some(CmdResult::NeedPoint);
        }
        let next=match keyword.as_str(){"S"|"STYLE"=>Some("style"),"P"|"PROPERTIES"=>Some("pattern"),"CO"|"COLOR"=>Some("color"),"LA"|"LAYER"=>Some("layer"),
            "AN"|"ANNOTATIVE"=>Some("annotative"),"T"|"TRANSPARENCY"=>Some("transparency"),"DR"|"DRAW"|"DRAW ORDER"=>Some("draworder"),
            "B"|"BOUNDARY"|"R"|"RECREATE"=>Some("boundary-type"),_=>None};
        if let Some(input)=next {self.input=Some(input);return Some(CmdResult::NeedPoint);}
        if matches!(keyword.as_str(),"H"|"HATCHES"|"SEPARATE") {return self.apply_result(HatchEditOperation::Separate);}
        if matches!(keyword.as_str(),"AS"|"ASSOCIATE"){return self.apply_result(HatchEditOperation::BeginAssociate);}
        if matches!(keyword.as_str(),"DI"|"DISASSOCIATE"){
            self.disassociate=true;return self.apply_result(self.update_operation());
        }
        let (_handle, name, scale, angle) = match &mut self.step {
            HatcheditStep::EditOptions {
                handle,
                name,
                scale,
                angle,
            } => (*handle, name, scale, angle),
            _ => return None,
        };

        let text = text.trim().to_uppercase();

        if text.is_empty() {
            return self.apply_result(self.update_operation());
        }

        if text == "ANNOTATIVE" {
            self.annotative = Some(!self.annotative.unwrap_or(self.annotative_current));
            return Some(CmdResult::NeedPoint);
        }
        if text == "SEPARATE" {
            return self.apply_result(HatchEditOperation::Separate);
        }

        // Parse option: P/S/A followed by value
        if let Some(rest) = text.strip_prefix('P') {
            let n = rest.trim().to_string();
            if !n.is_empty() {
                *name = n;
            }
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = text.strip_prefix('S') {
            if let Ok(v) = rest.trim().replace(',', ".").parse::<f32>() {
                if v > 0.0 {
                    *scale = v;
                }
            }
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = text.strip_prefix('A') {
            if let Ok(v) = rest.trim().replace(',', ".").parse::<f32>() {
                *angle = v;
            }
            return Some(CmdResult::NeedPoint);
        }

        if let Some(rest) = text.strip_prefix('O') {
            let values: Vec<_> = rest
                .trim()
                .split([',', ';', ' '])
                .filter(|part| !part.is_empty())
                .filter_map(|part| part.replace(',', ".").parse::<f64>().ok())
                .collect();
            if values.len() >= 2 {
                self.origin = Some((values[0], values[1]));
            }
            return Some(CmdResult::NeedPoint);
        }
        if text == "D" || text == "DISASSOCIATE" {
            self.disassociate = true;
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = text.strip_prefix('Y') {
            self.style = match rest.trim() {
                "NORMAL" | "N" => Some(acadrust::entities::HatchStyleType::Normal),
                "OUTER" | "O" => Some(acadrust::entities::HatchStyleType::Outer),
                "IGNORE" | "I" => Some(acadrust::entities::HatchStyleType::Ignore),
                _ => self.style,
            };
            return Some(CmdResult::NeedPoint);
        }
        if text == "N" || text == "ANNOTATIVE" {
            self.annotative = Some(!self.annotative.unwrap_or(self.annotative_current));
            return Some(CmdResult::NeedPoint);
        }
        if text == "R" || text == "RECREATE" {
            self.input=Some("boundary-type");return Some(CmdResult::NeedPoint);
        }
        if text == "E" || text == "SEPARATE" {
            return self.apply_result(HatchEditOperation::Separate);
        }
        if text == "F" || text == "FRONT" {
            return self.apply_result(HatchEditOperation::DrawOrderFront);
        }
        if text == "B" || text == "BACK" {
            return self.apply_result(HatchEditOperation::DrawOrderBack);
        }
        let parse_handles = |source: &str| {
            source
                .split([',', ';', ' '])
                .filter(|part| !part.is_empty())
                .filter_map(|part| {
                    u64::from_str_radix(part.trim_start_matches("0X"), 16)
                        .ok()
                        .map(Handle::new)
                })
                .collect::<Vec<_>>()
        };
        if let Some(rest) = text.strip_prefix('+') {
            return self.apply_result(HatchEditOperation::AddBoundaries(parse_handles(rest)));
        }
        if let Some(rest) = text.strip_prefix('-') {
            return self.apply_result(HatchEditOperation::RemoveBoundaries(parse_handles(rest)));
        }

        // Unrecognized — stay and re-prompt
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.input==Some("associate-point") {
            if let Some((plane,sources))=&self.association_sources {
                let point=plane.to_local(pt);
                let sources=sources.clone();
                if let Some(rings)=crate::scene::model::presspull_model::selected_rings(&sources,[point.x,point.y]) {
                    self.add_association_rings(rings,&sources);
                }else{self.association_missed=true;}
            }
        }
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        match self.input {
            None if matches!(self.step,HatcheditStep::EditOptions{..})=>{
                self.input=Some("pattern");CmdResult::NeedPoint
            }
            Some("style")=>{self.style=Some(self.style_current);self.apply_result(self.update_operation()).unwrap_or(CmdResult::Cancel)},
            Some("annotative")=>{self.annotative=Some(self.annotative_current);self.apply_result(self.update_operation()).unwrap_or(CmdResult::Cancel)},
            Some("pattern")=>{
                if matches!(&self.step,HatcheditStep::EditOptions{name,..} if name.eq_ignore_ascii_case("SOLID")) {
                    self.apply_result(self.update_operation()).unwrap_or(CmdResult::Cancel)
                }else{self.input=Some("scale");CmdResult::NeedPoint}
            }
            Some("scale")=>{self.input=Some("angle");CmdResult::NeedPoint}
            Some("angle")=>self.apply_result(self.update_operation()).unwrap_or(CmdResult::Cancel),
            Some("boundary-type")=>{self.boundary_region=false;self.input=Some("boundary-associate");CmdResult::NeedPoint},
            Some("boundary-associate")=>self.apply_result(HatchEditOperation::RecreateBoundary{associate:false,region:self.boundary_region}).unwrap_or(CmdResult::Cancel),
            Some("associate-select")=>{self.input=Some("associate-point");CmdResult::NeedPoint},
            Some("associate-point")=>if self.association_paths.is_empty(){CmdResult::Cancel}else{
                self.apply_result(HatchEditOperation::AssociatePaths(self.association_paths.clone())).unwrap_or(CmdResult::Cancel)
            },
            _=>CmdResult::Cancel,
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["HATCHEDIT"] });  // HatcheditCommand
