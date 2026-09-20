use acadrust::objects::{ObjectType, RasterVariables, WipeoutVariables};
use acadrust::{CadDocument, EntityType, Handle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameKind {
    Image,
    Pdf,
    Wipeout,
    Xclip,
    PointCloudClip,
}

pub(crate) const ALL_KINDS: [FrameKind; 5] = [
    FrameKind::Image,
    FrameKind::Pdf,
    FrameKind::Wipeout,
    FrameKind::Xclip,
    FrameKind::PointCloudClip,
];

fn root_entry(document: &CadDocument, name: &str) -> Option<Handle> {
    let from_dictionary = |handle| match document.objects.get(&handle) {
        Some(ObjectType::Dictionary(dictionary)) => dictionary
            .entries
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, handle)| *handle),
        _ => None,
    };
    from_dictionary(document.header.named_objects_dict_handle).or_else(|| {
        document.objects.values().find_map(|object| match object {
            ObjectType::Dictionary(dictionary) if dictionary.owner.is_null() => {
                from_dictionary(dictionary.handle)
            }
            _ => None,
        })
    })
}

fn raster_variables_handle(document: &CadDocument) -> Option<Handle> {
    root_entry(document, "ACAD_IMAGE_VARS").filter(|handle| {
        matches!(
            document.objects.get(handle),
            Some(ObjectType::RasterVariables(_))
        )
    })
}

fn wipeout_variables_handle(document: &CadDocument) -> Option<Handle> {
    root_entry(document, "ACAD_WIPEOUT_VARS").filter(|handle| {
        matches!(
            document.objects.get(handle),
            Some(ObjectType::WipeoutVariables(_))
        )
    })
}

fn drawing_mode(document: &CadDocument, name: &str, default: i16) -> i16 {
    crate::io::drawing_variable(document, name)
        .and_then(|value| value.parse::<i16>().ok())
        .unwrap_or(default)
        .clamp(0, 2)
}

pub(crate) fn mode(document: &CadDocument, kind: FrameKind) -> i16 {
    match kind {
        FrameKind::Image => raster_variables_handle(document)
            .and_then(|handle| match document.objects.get(&handle) {
                Some(ObjectType::RasterVariables(value)) => Some(value.display_image_frame),
                _ => None,
            })
            .unwrap_or(1)
            .clamp(0, 2),
        FrameKind::Pdf => drawing_mode(document, "PDFFRAME", 1),
        FrameKind::Wipeout => crate::io::drawing_variable(document, "WIPEOUTFRAME")
            .and_then(|value| value.parse::<i16>().ok())
            .or_else(|| {
                wipeout_variables_handle(document).and_then(|handle| {
                    match document.objects.get(&handle) {
                        Some(ObjectType::WipeoutVariables(value)) => Some(value.display_frame),
                        _ => None,
                    }
                })
            })
            .unwrap_or(1)
            .clamp(0, 2),
        FrameKind::Xclip => document.header.xclip_frame.clamp(0, 2),
        FrameKind::PointCloudClip => drawing_mode(document, "POINTCLOUDCLIPFRAME", 2),
    }
}

pub(crate) fn master_mode(document: &CadDocument) -> i16 {
    let first = mode(document, ALL_KINDS[0]);
    ALL_KINDS
        .iter()
        .skip(1)
        .all(|kind| mode(document, *kind) == first)
        .then_some(first)
        .unwrap_or(3)
}

fn attach_root_entry(document: &mut CadDocument, name: &str, handle: Handle) {
    let root = crate::scene::annotative::root_named_dict_handle(document);
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&root) {
        dictionary
            .entries
            .retain(|(key, _)| !key.eq_ignore_ascii_case(name));
        dictionary.add_entry(name, handle);
    }
}

fn set_image_mode(document: &mut CadDocument, value: i16) {
    let owner = crate::scene::annotative::root_named_dict_handle(document);
    let handle = raster_variables_handle(document).unwrap_or_else(|| {
        let handle = document.allocate_handle();
        let mut variables = RasterVariables::new();
        variables.handle = handle;
        variables.owner = owner;
        document
            .objects
            .insert(handle, ObjectType::RasterVariables(variables));
        handle
    });
    if let Some(ObjectType::RasterVariables(variables)) = document.objects.get_mut(&handle) {
        variables.owner = owner;
        variables.display_image_frame = value;
    }
    attach_root_entry(document, "ACAD_IMAGE_VARS", handle);
}

fn set_wipeout_mode(document: &mut CadDocument, value: i16) {
    crate::io::set_drawing_variable(document, "WIPEOUTFRAME", &value.to_string());
    let owner = crate::scene::annotative::root_named_dict_handle(document);
    let handle = wipeout_variables_handle(document).unwrap_or_else(|| {
        let handle = document.allocate_handle();
        let mut variables = WipeoutVariables::new();
        variables.handle = handle;
        variables.owner = owner;
        document
            .objects
            .insert(handle, ObjectType::WipeoutVariables(variables));
        handle
    });
    if let Some(ObjectType::WipeoutVariables(variables)) = document.objects.get_mut(&handle) {
        variables.owner = owner;
        variables.display_frame = value;
    }
    attach_root_entry(document, "ACAD_WIPEOUT_VARS", handle);
}

pub(crate) fn set_mode(document: &mut CadDocument, kind: FrameKind, value: i16) {
    let value = value.clamp(0, 2);
    match kind {
        FrameKind::Image => set_image_mode(document, value),
        FrameKind::Pdf => crate::io::set_drawing_variable(document, "PDFFRAME", &value.to_string()),
        FrameKind::Wipeout => set_wipeout_mode(document, value),
        FrameKind::Xclip => document.header.xclip_frame = value,
        FrameKind::PointCloudClip => {
            crate::io::set_drawing_variable(document, "POINTCLOUDCLIPFRAME", &value.to_string())
        }
    }
}

pub(crate) fn set_master_mode(document: &mut CadDocument, value: i16) {
    let value = value.clamp(0, 2);
    for kind in ALL_KINDS {
        if mode(document, kind) != value {
            set_mode(document, kind, value);
        }
    }
}

pub(crate) fn kind_for_name(name: &str) -> Option<FrameKind> {
    match name {
        "IMAGEFRAME" => Some(FrameKind::Image),
        "PDFFRAME" => Some(FrameKind::Pdf),
        "WIPEOUTFRAME" => Some(FrameKind::Wipeout),
        "XCLIPFRAME" => Some(FrameKind::Xclip),
        "POINTCLOUDCLIPFRAME" => Some(FrameKind::PointCloudClip),
        _ => None,
    }
}

pub(crate) fn entity_kind(entity: &EntityType) -> Option<FrameKind> {
    match entity {
        EntityType::RasterImage(_) => Some(FrameKind::Image),
        EntityType::Underlay(underlay)
            if matches!(
                underlay.underlay_type,
                acadrust::entities::UnderlayType::Pdf
            ) =>
        {
            Some(FrameKind::Pdf)
        }
        EntityType::Wipeout(_) => Some(FrameKind::Wipeout),
        _ => None,
    }
}

pub(crate) fn affected(entity: &EntityType, kind: FrameKind) -> bool {
    match kind {
        FrameKind::Image => matches!(entity, EntityType::RasterImage(_)),
        FrameKind::Pdf => matches!(
            entity,
            EntityType::Underlay(underlay)
                if matches!(underlay.underlay_type, acadrust::entities::UnderlayType::Pdf)
        ),
        FrameKind::Wipeout => matches!(entity, EntityType::Wipeout(_)),
        FrameKind::Xclip => matches!(entity, EntityType::Insert(_)),
        FrameKind::PointCloudClip => matches!(
            entity,
            EntityType::Extended(extended)
                if matches!(
                    &extended.data,
                    acadrust::entities::ExtendedEntityData::PointCloud(_)
                        | acadrust::entities::ExtendedEntityData::PointCloudEx(_)
                )
        ),
    }
}
