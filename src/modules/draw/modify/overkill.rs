use acadrust::{EntityType, Handle};
use glam::DVec3;
use crate::command::{CadCommand, CmdOption, CmdResult};

#[derive(Clone, Copy)]
pub struct Settings {
    pub tolerance: f64,
    pub ignore: u16,
    pub optimize: bool,
    pub overlap: bool,
    pub end_to_end: bool,
    pub associativity: bool,
}
impl Default for Settings {
    fn default() -> Self { Self { tolerance: 1e-6, ignore: 0, optimize: true,
        overlap: true, end_to_end: true, associativity: true } }
}
fn remembered() -> &'static std::sync::Mutex<Settings> {
    static SETTINGS: std::sync::OnceLock<std::sync::Mutex<Settings>> = std::sync::OnceLock::new();
    SETTINGS.get_or_init(|| std::sync::Mutex::new(Settings::default()))
}
enum Step { Selection, Options, Ignore, Tolerance, Optimize, Overlap, EndToEnd, Associativity }
pub struct OverkillCommand { selected: Vec<Handle>, step: Step, settings: Settings }
impl OverkillCommand {
    pub fn new(selected: Vec<Handle>) -> Self {
        let step = if selected.is_empty() { Step::Selection } else { Step::Options };
        Self { selected, step, settings: *remembered().lock().unwrap_or_else(|e|e.into_inner()) }
    }
    fn finish(&self) -> CmdResult {
        let s = self.settings;
        *remembered().lock().unwrap_or_else(|e|e.into_inner())=s;
        CmdResult::Relaunch(format!("OVERKILL_APPLY {} {} {} {} {} {}", s.tolerance,
            s.ignore, u8::from(s.optimize), u8::from(s.overlap), u8::from(s.end_to_end),
            u8::from(s.associativity)), self.selected.clone())
    }
}
impl CadCommand for OverkillCommand {
    fn name(&self) -> &'static str { "OVERKILL" }
    fn prompt(&self) -> String {
        match self.step {
            Step::Selection => "Select objects:".into(),
            Step::Options => format!("Tolerance={}  Ignore={}  Optimize={}  Overlap={}  End-to-end={}\nEnter an option [Done/Ignore/tOlerance/optimize Plines/combine parTial overlap/combine Endtoend/Associativity] <Done>:", self.settings.tolerance, self.settings.ignore, self.settings.optimize, self.settings.overlap, self.settings.end_to_end),
            Step::Ignore => "Properties to ignore [None/All/Color/LAyer/Ltype/ltScale/LWeight/Thickness/TRansparency/plotSTyle/Material]:".into(),
            Step::Tolerance => format!("Specify tolerance <{}>:", self.settings.tolerance),
            Step::Optimize => format!("Optimize segments within polylines [Yes/No] <{}>:",if self.settings.optimize{"Yes"}else{"No"}),
            Step::Overlap => format!("Combine partially overlapping objects [Yes/No] <{}>:",if self.settings.overlap{"Yes"}else{"No"}),
            Step::EndToEnd => format!("Combine end-to-end objects [Yes/No] <{}>:",if self.settings.end_to_end{"Yes"}else{"No"}),
            Step::Associativity => format!("Maintain associative objects [Yes/No] <{}>:",if self.settings.associativity{"Yes"}else{"No"}),
        }
    }
    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Options => [("Done","D"),("Ignore","I"),("Tolerance","O"),("Optimize polylines","P"),("Combine overlap","T"),("Combine end-to-end","E"),("Associativity","A")].into_iter().map(|(l,k)|CmdOption::new(l,k)).collect(),
            Step::Optimize|Step::Overlap|Step::EndToEnd|Step::Associativity => vec![CmdOption::new("Yes","Y"),CmdOption::new("No","N")],
            _ => Vec::new(),
        }
    }
    fn is_selection_gathering(&self) -> bool { matches!(self.step,Step::Selection) }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult { self.selected=handles; CmdResult::NeedPoint }
    fn wants_text_input(&self) -> bool { !self.is_selection_gathering() }
    fn on_point(&mut self, _:DVec3) -> CmdResult { CmdResult::NeedPoint }
    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Selection if self.selected.is_empty() => CmdResult::Cancel,
            Step::Options => self.finish(),
            _ => { self.step=Step::Options; CmdResult::NeedPoint }
        }
    }
    fn on_text_input(&mut self, text:&str) -> Option<CmdResult> {
        let k=text.trim().to_ascii_uppercase();
        match self.step {
            Step::Options => match k.as_str() {
                "D"|"DONE" => return Some(self.finish()),
                "I"|"IGNORE" => self.step=Step::Ignore,
                "O"|"TOLERANCE" => self.step=Step::Tolerance,
                "P"|"PLINES" => self.step=Step::Optimize,
                "T"|"PARTIAL" => self.step=Step::Overlap,
                "E"|"ENDTOEND" => self.step=Step::EndToEnd,
                "A"|"ASSOCIATIVITY" => self.step=Step::Associativity,
                _=>{},
            },
            Step::Ignore => {
                let flag=match k.as_str() { "N"|"NONE"=>{self.settings.ignore=0;0}, "A"|"ALL"=>511,
                    "C"|"COLOR"=>1,"LA"|"LAYER"=>2,"L"|"LTYPE"=>4,"S"|"LTSCALE"=>8,
                    "LW"|"LWEIGHT"=>16,"T"|"THICKNESS"=>32,"TR"|"TRANSPARENCY"=>64,
                    "ST"|"PLOTSTYLE"=>128,"M"|"MATERIAL"=>256,_=>return Some(CmdResult::NeedPoint)};
                self.settings.ignore|=flag; self.step=Step::Options;
            }
            Step::Tolerance => if let Ok(v)=k.parse::<f64>() { if v.is_finite()&&v>=0.0 {
                self.settings.tolerance=v; self.step=Step::Options;
            } },
            Step::Optimize|Step::Overlap|Step::EndToEnd|Step::Associativity => {
                let yes=match k.as_str(){"Y"|"YES"=>true,"N"|"NO"=>false,_=>return Some(CmdResult::NeedPoint)};
                match self.step {Step::Optimize=>self.settings.optimize=yes,Step::Overlap=>self.settings.overlap=yes,
                    Step::EndToEnd=>self.settings.end_to_end=yes,Step::Associativity=>self.settings.associativity=yes,_=>{}}
                self.step=Step::Options;
            }
            _=>{},
        }
        Some(CmdResult::NeedPoint)
    }
}

pub fn normalized(entity:&EntityType, ignore:u16) -> EntityType {
    let mut e=entity.clone();
    let common=e.common_mut();
    let default=acadrust::entities::EntityCommon::new();
    common.handle=Handle::NULL;
    if ignore&1!=0 {common.color=default.color;common.color_name=None;common.color_book_handle=None;}
    if ignore&2!=0 {common.layer.clear();}
    if ignore&4!=0 {common.linetype.clear();common.linetype_handle=None;}
    if ignore&8!=0 {common.linetype_scale=1.0;}
    if ignore&16!=0 {common.line_weight=default.line_weight;}
    if ignore&64!=0 {common.transparency=default.transparency;}
    if ignore&128!=0 {common.plotstyle_flags=0;common.plotstyle_handle=None;}
    if ignore&256!=0 {common.material_flags=0;common.material_handle=None;}
    if ignore&32!=0 {match &mut e {EntityType::Line(v)=>v.thickness=0.0,
        EntityType::Circle(v)=>v.thickness=0.0,EntityType::Arc(v)=>v.thickness=0.0,
        EntityType::LwPolyline(v)=>v.thickness=0.0,_=>{}}}
    e
}

pub fn optimize(entity:&mut EntityType, tolerance:f64) {
    if let EntityType::LwPolyline(p)=entity {
        // Width changes and bulged spans carry geometry that must be retained.
        if p.vertices.iter().any(|v|v.bulge!=0.0||v.start_width!=0.0||v.end_width!=0.0) {return;}
        let points:Vec<_>=p.vertices.iter().map(|v|[v.location.x,v.location.y,p.elevation]).collect();
        let indices=cadkernel::space::simplify_linear_chain(&points,tolerance);
        if indices.len()>=if p.is_closed{3}else{2} {
            p.vertices=indices.into_iter().map(|i|p.vertices[i].clone()).collect();
        }
    }
}
