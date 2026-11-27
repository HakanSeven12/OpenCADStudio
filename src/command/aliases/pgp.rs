//! AutoCAD PGP interoperability. Never interpret external-command definitions.
use std::collections::BTreeMap;

/// UTF-8 (including BOM), or the Windows-1252 encoding used by legacy acad.pgp.
pub(crate) fn decode(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.trim_start_matches('\u{feff}').to_owned();
    }
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9f => HIGH[(b - 0x80) as usize],
            _ => char::from(b),
        })
        .collect()
}

/// Last definition wins, as in AutoCAD's user-defined aliases section.
/// Accept OpenCAD's optional star too, but reject all multi-field shell rows.
pub(crate) fn parse(body: &str) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    for line in body.trim_start_matches('\u{feff}').lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        let Some((alias, target)) = line.split_once(',') else {
            continue;
        };
        let alias = alias.trim().to_uppercase();
        let target = target
            .trim()
            .strip_prefix('*')
            .unwrap_or(target.trim())
            .trim()
            .to_uppercase();
        if alias.is_empty()
            || target.is_empty()
            || target.contains(',')
            || alias.chars().any(char::is_whitespace)
            || target.chars().any(char::is_whitespace)
        {
            continue;
        }
        aliases.insert(alias, target);
    }
    aliases
}

/// Translate command names, never alias keys (e.g. the user's E means FILLET).
pub(crate) fn canonical(target: &str) -> String {
    let target = target.trim_start_matches(['_', '.']);
    let (prefix, name) = target.strip_prefix('-').map_or(("", target), |s| ("-", s));
    let name = match name {
        "LINEA" => "LINE",
        "POL" | "POLILINEA" => "PLINE",
        "3DPOL" => "3DPOLY",
        "CIRCULO" => "CIRCLE",
        "ARCO" => "ARC",
        "RECTANG" => "RECTANG",
        "ELIPSE" => "ELLIPSE",
        "POLIGONO" => "POLYGON",
        "ARANDELA" => "DONUT",
        "PUNTO" => "POINT",
        "COPIA" => "COPY",
        "DESPLAZA" => "MOVE",
        "DESFASE" => "OFFSET",
        "GIRA" => "ROTATE",
        "SIMETRIA" => "MIRROR",
        "ESCALA" => "SCALE",
        "ESTIRA" => "STRETCH",
        "RECORTA" => "TRIM",
        "ALARGA" => "EXTEND",
        "EMPALME" => "FILLET",
        "CHAFLAN" => "CHAMFER",
        "BORRA" => "ERASE",
        "DESCOMP" => "EXPLODE",
        "JUNTAR" | "UNIR" => "JOIN",
        "PARTE" => "BREAK",
        "PARTIRENPUNTO" => "BREAKATPOINT",
        "MATRIZ" => "ARRAY",
        "ALINEAR" => "ALIGN",
        "LONGITUD" => "LENGTHEN",
        "EDITPOL" => "PEDIT",
        "SOMBREA" => "HATCH",
        "EDITSOMB" => "HATCHEDIT",
        "CONTORNO" => "BOUNDARY",
        "CAPA" => "LAYER",
        "CAMBCAPAMULT" => "LAYMCH",
        "IGUALARPROP" => "MATCHPROP",
        "PROPIEDADES" => "PROPERTIES",
        "CAMBIA" => "CHANGE",
        "TIPOLIN" => "LINETYPE",
        "GROSORLIN" => "LWEIGHT",
        "ESCALATL" => "LTSCALE",
        "TEXTO" => "TEXT",
        "TEXTOM" => "MTEXT",
        "EDITARTEXTO" => "TEXTEDIT",
        "EDITTXTM" => "MTEDIT",
        "ESTILO" => "STYLE",
        "BUSCAR" => "FIND",
        "BLOQUE" => "BLOCK",
        "BLOQUEDISC" => "WBLOCK",
        "MODBLQ" => "BEDIT",
        "ATRDEF" => "ATTDEF",
        "ATREDIT" => "ATTEDIT",
        "ATREXT" => "ATTEXT",
        "ACOALINEADA" => "DIMALIGNED",
        "ACOANGULO" => "DIMANGULAR",
        "ACOLINEAL" => "DIMLINEAR",
        "ACORADIO" => "DIMRADIUS",
        "ACODIAMETRO" => "DIMDIAMETER",
        "ACOCOORDENADA" => "DIMORDINATE",
        "ACOLINEABASE" => "DIMBASELINE",
        "ACOCONTINUA" => "DIMCONTINUE",
        "ACOESTIL" => "DIMSTYLE",
        "ACOEDIC" => "DIMEDIT",
        "ACOTEDIC" => "DIMTEDIT",
        "DIRECTRIZ" => "LEADER",
        "DIRECTRIZR" => "QLEADER",
        "SCP" => "UCS",
        "ADMINSCP" => "UCSMAN",
        "VISTA" => "VIEW",
        "VISTADIN" => "DVIEW",
        "3DORBITA" => "3DORBIT",
        "3DCARA" => "3DFACE",
        "ENCUADRE" => "PAN",
        "REDIBUJA" => "REDRAW",
        "REDIBT" => "REDRAWALL",
        "REGENERA" => "REGEN",
        "REGENT" => "REGENALL",
        "OCULTA" => "HIDE",
        "ABRE" => "OPEN",
        "NUEVO" => "NEW",
        "GUARDAR" => "QSAVE",
        "GUARDARCOMO" => "SAVEAS",
        "TRAZAR" => "PLOT",
        "IMPORTAR" => "IMPORT",
        "EXPORTAR" => "EXPORT",
        "EXPORTARPDF" => "EXPORTPDF",
        "OPCIONES" => "OPTIONS",
        "UNIDADES" => "UNITS",
        "PARAMSDIB" => "DSETTINGS",
        "DIST" => "DIST",
        "LIST" => "LIST",
        "EXTRUSION" => "EXTRUDE",
        "REVOLUCION" => "REVOLVE",
        "INTERSEC" => "INTERSECT",
        "DIFERENCIA" => "SUBTRACT",
        "REGION" => "REGION",
        "ORDENAOBJETOS" => "DRAWORDER",
        "IMAGEN" => "IMAGE",
        "ENLAZARIMG" => "IMAGEATTACH",
        "DELIMITARIMG" => "IMAGECLIP",
        "AJUSTARIMG" => "IMAGEADJUST",
        "REFX" => "XREF",
        "ESPACIOM" => "MSPACE",
        "ESPACIOP" => "PSPACE",
        "VMULT" => "MVIEW",
        "REFENT" => "OSNAP",
        "PEGAESP" => "PASTESPEC",
        "CERPROP" => "PROPERTIESCLOSE",
        "PREVISUALIZAR" => "PREVIEW",
        "LIMPIA" => "PURGE",
        "RENOMBRA" => "RENAME",
        "MODIVAR" => "SETVAR",
        "MODOSOMBRA" => "SHADEMODE",
        "SECCION" => "SECTION",
        "CORTE" => "SLICE",
        "FORZCURSOR" => "SNAP",
        "ORTOGRAFIA" => "SPELL",
        "EDITSPLINE" => "SPLINEDIT",
        "TOLERANCIA" => "TOLERANCE",
        "TOROIDE" => "TORUS",
        "PTOVISTA" => "VPOINT",
        "CUÑA" => "WEDGE",
        "LINEAX" => "XLINE",
        "ENLAZARX" => "XATTACH",
        "UNIRX" => "XBIND",
        "DELIMITARX" => "XCLIP",
        "TABLA" => "TABLE",
        "ESTILOTABLA" => "TABLESTYLE",
        "MARCAREV" => "REVCLOUD",
        "CERRARBLOQUE" => "BCLOSE",
        "EDITARBLOQUE" => "BEDIT",
        "GUARDARBLOQUE" => "BSAVE",
        "ACOARCO" => "DIMARC",
        "ACORECODO" => "DIMJOGGED",
        "DEGRADADO" => "GRADIENT",
        "RECUPDIBUJO" => "RECOVER",
        "CALCURAPIDA" => "QUICKCALC",
        "ESTADOVISBLOQUE" => "BVSTATE",
        "SIMETRIA3D" => "MIRROR3D",
        "DESPLAZA3D" => "3DMOVE",
        "GIRA3D" => "3DROTATE",
        "ESCALA3D" => "3DSCALE",
        "ALINEAR3D" => "3DALIGN",
        "CAMARA" => "CAMERA",
        "CILINDRO" => "CYLINDER",
        "REFEXTERNAS" => "EXTERNALREFERENCES",
        "GEOPLANA" => "FLATSHOT",
        "POLISOLIDO" => "POLYSOLID",
        "PIRAMIDE" => "PYRAMID",
        "PLANOSECCION" => "SECTIONPLANE",
        "ESTILVISUAL" => "VISUALSTYLES",
        "ESTVISACTUAL" => "VSCURRENT",
        "EDITATRINSITU" => "ATTIPEDIT",
        "EXTRACDAT" => "DATAEXTRACTION",
        "ESTADOCAPAS" => "LAYERSTATE",
        "LUZPUNTUAL" => "POINTLIGHT",
        "DIRECTRIZM" => "MLEADER",
        "ESTILDIRECTRIZM" => "MLEADERSTYLE",
        "EDITARDIRECTRIZM" => "MLEADEREDIT",
        "ALINDIRECTRIZM" => "MLEADERALIGN",
        "COLECDIRECTRIZM" => "MLEADERCOLLECT",
        "MEDIRGEOM" => "MEASUREGEOM",
        "PARAMETROS" => "PARAMETERS",
        "TIPOPUNTO" => "PTYPE",
        "ALINEARTEXTO" => "TEXTALIGN",
        "ENLAZARNUBEPUNTOS" => "POINTCLOUDATTACH",
        "RECORTANUBEPUNTOS" => "POINTCLOUDCLIP",
        "QUITA" => "REMOVE",
        "ACOCENTRO" => "DIMCENTER",
        "ACOREMPLAZAR" => "DIMOVERRIDE",
        "GRADUA" => "MEASURE",
        "LINEAM" => "MLINE",
        "GRUPO" => "GROUP",
        "FILTRO" => "FILTER",
        "UNION" => "UNION",
        "INSERTOBJ" => "INSERTOBJ",
        _ => name,
    };
    format!("{prefix}{name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_overrides_and_external_commands() {
        let rows = parse(concat!(
            "\u{feff}; header\r\n",
            "B,*BORRA\r\n",
            "NOTEPAD, START NOTEPAD,1,*File:,\n",
            "B,*BUSCAR ; custom\n",
            "E,*EMPALME\n",
            "BAD,*LINE,extra\n",
            "L,LINE\n",
        ));
        assert_eq!(rows.len(), 3);
        assert_eq!(canonical(&rows["B"]), "FIND");
        assert_eq!(canonical(&rows["E"]), "FILLET");
        assert_eq!(rows["L"], "LINE");
    }
    #[test]
    fn localized_targets_and_prefixes() {
        for (input, expected) in [
            ("_LINE", "LINE"),
            ("._-CAPA", "-LAYER"),
            ("COPIA", "COPY"),
            ("SCP", "UCS"),
            ("AECCUNKNOWN", "AECCUNKNOWN"),
        ] {
            assert_eq!(canonical(input), expected);
        }
    }
    #[test]
    fn legacy_encoding_and_utf8_bom() {
        assert_eq!(
            decode(b"; par\xe1metros\nL,*LINEA"),
            "; parámetros\nL,*LINEA"
        );
        assert_eq!(decode("\u{feff}L,*LINEA".as_bytes()), "L,*LINEA");
    }
}
