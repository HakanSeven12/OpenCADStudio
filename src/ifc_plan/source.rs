use document::entities::EntityCommon;
use document::xdata::XDataValue;

pub(crate) const APPLICATION: &str = "OPENAEC_IFC_SOURCE";

pub(crate) struct Source<'a> {
    pub file: Option<&'a str>,
    pub elements: Vec<Element<'a>>,
}

pub(crate) struct Element<'a> {
    pub step_id: u32,
    pub ifc_type: &'a str,
    pub global_id: &'a str,
}

pub(crate) fn read(common: &EntityCommon) -> Option<Source<'_>> {
    let record = common.extended_data.get_record(APPLICATION)?;
    let mut strings = record.values.iter().filter_map(|value| match value {
        XDataValue::String(s) => Some(s.as_str()),
        _ => None,
    });
    if strings.next() != Some("v1") {
        return None;
    }
    let mut file = None;
    let mut elements = Vec::new();
    for value in strings {
        if let Some(name) = value.strip_prefix("file=").filter(|name| !name.is_empty()) {
            file = Some(name);
        } else if let Some(value) = value.strip_prefix("ifc=") {
            let mut fields = value.splitn(3, '|');
            if let (Some(Ok(step_id)), Some(ifc_type), Some(global_id)) =
                (fields.next().map(str::parse), fields.next(), fields.next())
            {
                elements.push(Element {
                    step_id,
                    ifc_type,
                    global_id,
                });
            }
        }
    }
    (!elements.is_empty()).then_some(Source { file, elements })
}
