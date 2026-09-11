//! DXF group codes
//!
//! Group codes define the type of data that follows in a DXF file.
//! Each code indicates what kind of value to expect (string, integer, float, etc.)

/// DXF group codes
///
/// These codes appear in DXF files to indicate the type of data that follows.
/// The codes are organized by range, with each range representing a specific data type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i32)]
pub enum DxfCode {
    /// Invalid code
    Invalid = -9999,

    /// Extended dictionary (XDICTIONARY)
    XDictionary = -6,

    /// Persistent reactor chain
    PReactors = -5,

    /// Conditional operator (used only with ssget)
    Operator = -4,

    /// Extended data (XDATA) sentinel (fixed)
    XDataStart = -3,

    /// Header ID / First entity ID / entity name reference (fixed)
    HeaderId = -2,

    /// Entity name (changes each time drawing is opened, never saved)
    End = -1,

    // ===== 0-9: String values =====
    /// Text string indicating the entity type (fixed)
    Start = 0,

    /// Primary text value for an entity / XRef path name
    Text = 1,

    /// Name (attribute tag, block name, etc.)
    Name = 2,

    /// Other text or name values
    OtherName = 3,
    TextStyleName = 7,
    LayerName = 8,
    CLShapeText = 9,

    // ===== 10-59: Floating-point values (coordinates, distances, etc.) =====
    /// Primary X coordinate
    XCoordinate = 10,
    /// Primary Y coordinate  
    YCoordinate = 20,
    /// Primary Z coordinate
    ZCoordinate = 30,

    /// Secondary X coordinate
    XCoordinate1 = 11,
    /// Secondary Y coordinate
    YCoordinate1 = 21,
    /// Secondary Z coordinate
    ZCoordinate1 = 31,

    /// Tertiary X coordinate
    XCoordinate2 = 12,
    /// Tertiary Y coordinate
    YCoordinate2 = 22,
    /// Tertiary Z coordinate
    ZCoordinate2 = 32,

    /// Quaternary X coordinate
    XCoordinate3 = 13,
    /// Quaternary Y coordinate
    YCoordinate3 = 23,
    /// Quaternary Z coordinate
    ZCoordinate3 = 33,

    /// Additional coordinates (14-18, 24-28, 34-38)
    XCoordinate4 = 14,
    YCoordinate4 = 24,
    ZCoordinate4 = 34,

    XCoordinate5 = 15,
    YCoordinate5 = 25,
    ZCoordinate5 = 35,

    XCoordinate6 = 16,
    YCoordinate6 = 26,
    ZCoordinate6 = 36,

    XCoordinate7 = 17,
    YCoordinate7 = 27,
    ZCoordinate7 = 37,

    XCoordinate8 = 18,
    YCoordinate8 = 28,
    /// Z coordinate (8th) / Elevation
    ZCoordinate8 = 38,

    /// Thickness
    Thickness = 39,

    /// Floating-point values (40-59)
    Real40 = 40,
    Real41 = 41,
    Real42 = 42,
    Real43 = 43,
    Real44 = 44,
    Real45 = 45,
    Real46 = 46,
    Real47 = 47,
    Real48 = 48,
    Real49 = 49,

    /// Angles (50-58)
    Angle50 = 50,
    Angle51 = 51,
    Angle52 = 52,
    Angle53 = 53,
    Angle54 = 54,
    Angle55 = 55,
    Angle56 = 56,
    Angle57 = 57,
    Angle58 = 58,

    // ===== 60-79: Integer values =====
    /// Visibility (0 = visible, 1 = invisible)
    Visibility = 60,

    /// Parameter space curve type
    ParameterSpaceCurveType = 61,

    /// Color number
    Color = 62,

    /// Entities follow flag
    EntitiesFollow = 66,

    /// Model/paper space flag
    ModelSpace = 67,

    /// Viewport status field
    ViewportStatus = 68,

    /// Viewport ID
    ViewportId = 69,

    /// Integer values (70-79)
    Int70 = 70,
    Int71 = 71,
    Int72 = 72,
    Int73 = 73,
    Int74 = 74,
    Int75 = 75,
    Int76 = 76,
    Int77 = 77,
    Int78 = 78,
    Int79 = 79,

    // ===== 90-99: 32-bit integer values =====
    Int90 = 90,
    Int91 = 91,
    Int92 = 92,
    Int93 = 93,
    Int94 = 94,
    Int95 = 95,
    Int96 = 96,
    Int97 = 97,
    Int98 = 98,
    Int99 = 99,

    // ===== 100-109: Subclass markers and strings =====
    /// Subclass data marker
    SubclassMarker = 100,

    /// Control string
    ControlString = 101,

    /// UCS origin (X coordinate)
    UcsOriginX = 110,
    /// UCS origin (Y coordinate)
    UcsOriginY = 120,
    /// UCS origin (Z coordinate)
    UcsOriginZ = 130,

    /// UCS X-axis (X coordinate)
    UcsXAxisX = 111,
    /// UCS X-axis (Y coordinate)
    UcsXAxisY = 121,
    /// UCS X-axis (Z coordinate)
    UcsXAxisZ = 131,

    /// UCS Y-axis (X coordinate)
    UcsYAxisX = 112,
    /// UCS Y-axis (Y coordinate)
    UcsYAxisY = 122,
    /// UCS Y-axis (Z coordinate)
    UcsYAxisZ = 132,

    // ===== 140-149: Double precision floating-point values =====
    Real140 = 140,
    Real141 = 141,
    Real142 = 142,
    Real143 = 143,
    Real144 = 144,
    Real145 = 145,
    Real146 = 146,
    Real147 = 147,
    Real148 = 148,
    Real149 = 149,

    // ===== 160-169: 64-bit integer values =====
    Int160 = 160,
    Int161 = 161,
    Int162 = 162,
    Int163 = 163,
    Int164 = 164,
    Int165 = 165,
    Int166 = 166,
    Int167 = 167,
    Int168 = 168,
    Int169 = 169,

    // ===== 170-179: 16-bit integer values =====
    Int170 = 170,
    Int171 = 171,
    Int172 = 172,
    Int173 = 173,
    Int174 = 174,
    Int175 = 175,
    Int176 = 176,
    Int177 = 177,
    Int178 = 178,
    Int179 = 179,

    // ===== 210-239: Extrusion direction and other vectors =====
    /// Extrusion direction X
    ExtrusionX = 210,
    /// Extrusion direction Y
    ExtrusionY = 220,
    /// Extrusion direction Z
    ExtrusionZ = 230,

    // ===== 270-289: 8-bit integer values =====
    Int270 = 270,
    Int271 = 271,
    Int272 = 272,
    Int273 = 273,
    Int274 = 274,
    Int275 = 275,
    Int276 = 276,
    Int277 = 277,
    Int278 = 278,
    Int279 = 279,

    Int280 = 280,
    Int281 = 281,
    Int282 = 282,
    Int283 = 283,
    Int284 = 284,
    Int285 = 285,
    Int286 = 286,
    Int287 = 287,
    Int288 = 288,
    Int289 = 289,

    // ===== 290-299: Boolean values =====
    Bool290 = 290,
    Bool291 = 291,
    Bool292 = 292,
    Bool293 = 293,
    Bool294 = 294,
    Bool295 = 295,
    Bool296 = 296,
    Bool297 = 297,
    Bool298 = 298,
    Bool299 = 299,

    // ===== 300-309: Arbitrary text strings =====
    Text300 = 300,
    Text301 = 301,
    Text302 = 302,
    Text303 = 303,
    Text304 = 304,
    Text305 = 305,
    Text306 = 306,
    Text307 = 307,
    Text308 = 308,
    Text309 = 309,

    // ===== 310-319: Binary data =====
    BinaryData310 = 310,
    BinaryData311 = 311,
    BinaryData312 = 312,
    BinaryData313 = 313,
    BinaryData314 = 314,
    BinaryData315 = 315,
    BinaryData316 = 316,
    BinaryData317 = 317,
    BinaryData318 = 318,
    BinaryData319 = 319,

    // ===== 320-329: Arbitrary object handles =====
    Handle320 = 320,
    Handle321 = 321,
    Handle322 = 322,
    Handle323 = 323,
    Handle324 = 324,
    Handle325 = 325,
    Handle326 = 326,
    Handle327 = 327,
    Handle328 = 328,
    Handle329 = 329,

    // ===== 330-339: Soft-pointer handle =====
    SoftPointerId330 = 330,
    SoftPointerId331 = 331,
    SoftPointerId332 = 332,
    SoftPointerId333 = 333,
    SoftPointerId334 = 334,
    SoftPointerId335 = 335,
    SoftPointerId336 = 336,
    SoftPointerId337 = 337,
    SoftPointerId338 = 338,
    SoftPointerId339 = 339,

    // ===== 340-349: Hard-pointer handle =====
    HardPointerId340 = 340,
    HardPointerId341 = 341,
    HardPointerId342 = 342,
    HardPointerId343 = 343,
    HardPointerId344 = 344,
    HardPointerId345 = 345,
    HardPointerId346 = 346,
    HardPointerId347 = 347,
    HardPointerId348 = 348,
    HardPointerId349 = 349,

    // ===== 350-359: Soft-owner handle =====
    SoftOwnerId350 = 350,
    SoftOwnerId351 = 351,
    SoftOwnerId352 = 352,
    SoftOwnerId353 = 353,
    SoftOwnerId354 = 354,
    SoftOwnerId355 = 355,
    SoftOwnerId356 = 356,
    SoftOwnerId357 = 357,
    SoftOwnerId358 = 358,
    SoftOwnerId359 = 359,

    // ===== 360-369: Hard-owner handle =====
    HardOwnerId360 = 360,
    HardOwnerId361 = 361,
    HardOwnerId362 = 362,
    HardOwnerId363 = 363,
    HardOwnerId364 = 364,
    HardOwnerId365 = 365,
    HardOwnerId366 = 366,
    HardOwnerId367 = 367,
    HardOwnerId368 = 368,
    HardOwnerId369 = 369,

    // ===== 370-379: Lineweight and plot style =====
    /// Lineweight enum value
    Lineweight = 370,

    /// Plot style name type
    PlotStyleNameType = 380,

    /// Plot style name ID/handle
    PlotStyleNameId = 390,

    // ===== 400-409: 16-bit integers =====
    Int400 = 400,
    Int401 = 401,
    Int402 = 402,
    Int403 = 403,
    Int404 = 404,
    Int405 = 405,
    Int406 = 406,
    Int407 = 407,
    Int408 = 408,
    Int409 = 409,

    // ===== 410-419: String values =====
    LayoutName = 410,

    // ===== 420-429: 32-bit integer color values =====
    TrueColor = 420,
    ColorName = 430,

    /// Transparency value
    Transparency = 440,

    // ===== 450-459: Long values =====
    Int450 = 450,
    Int451 = 451,
    Int452 = 452,
    Int453 = 453,
    Int454 = 454,
    Int455 = 455,
    Int456 = 456,
    Int457 = 457,
    Int458 = 458,
    Int459 = 459,

    // ===== 460-469: Double values =====
    Real460 = 460,
    Real461 = 461,
    Real462 = 462,
    Real463 = 463,
    Real464 = 464,
    Real465 = 465,
    Real466 = 466,
    Real467 = 467,
    Real468 = 468,
    Real469 = 469,

    // ===== 470-479: String values =====
    Text470 = 470,
    Text471 = 471,
    Text472 = 472,
    Text473 = 473,
    Text474 = 474,
    Text475 = 475,
    Text476 = 476,
    Text477 = 477,
    Text478 = 478,
    Text479 = 479,

    // ===== 480-481: Hard-pointer handle values =====
    Handle480 = 480,
    Handle481 = 481,

    // ===== 999: Comment =====
    /// Comment (string)
    Comment = 999,

    // ===== 1000-1071: Extended data (XDATA) =====
    /// Extended data string (255-byte maximum)
    XDataString = 1000,

    /// Extended data registered application name
    XDataAppName = 1001,

    /// Extended data control string
    XDataControlString = 1002,

    /// Extended data layer name
    XDataLayerName = 1003,

    /// Extended data binary chunk
    XDataBinaryChunk = 1004,

    /// Extended data database handle
    XDataHandle = 1005,

    /// Extended data 3D point
    XDataXCoordinate = 1010,
    XDataYCoordinate = 1020,
    XDataZCoordinate = 1030,

    /// Extended data world space position
    XDataWorldXCoordinate = 1011,
    XDataWorldYCoordinate = 1021,
    XDataWorldZCoordinate = 1031,

    /// Extended data world space displacement
    XDataWorldXDisplacement = 1012,
    XDataWorldYDisplacement = 1022,
    XDataWorldZDisplacement = 1032,

    /// Extended data world direction
    XDataWorldXDirection = 1013,
    XDataWorldYDirection = 1023,
    XDataWorldZDirection = 1033,

    /// Extended data real value
    XDataReal = 1040,

    /// Extended data distance value
    XDataDistance = 1041,

    /// Extended data scale factor
    XDataScaleFactor = 1042,

    /// Extended data integer (16-bit signed)
    XDataInteger16 = 1070,

    /// Extended data integer (32-bit signed)
    XDataInteger32 = 1071,
}

impl DxfCode {
    /// Convert an integer code to a DxfCode enum value
    pub fn from_i32(code: i32) -> Self {
        match code {
            -9999 => DxfCode::Invalid,
            -6 => DxfCode::XDictionary,
            -5 => DxfCode::PReactors,
            -4 => DxfCode::Operator,
            -3 => DxfCode::XDataStart,
            -2 => DxfCode::HeaderId,
            -1 => DxfCode::End,
            0 => DxfCode::Start,
            1 => DxfCode::Text,
            2 => DxfCode::Name,
            3 => DxfCode::OtherName,
            7 => DxfCode::TextStyleName,
            8 => DxfCode::LayerName,
            9 => DxfCode::CLShapeText,
            10 => DxfCode::XCoordinate,
            20 => DxfCode::YCoordinate,
            30 => DxfCode::ZCoordinate,
            11 => DxfCode::XCoordinate1,
            21 => DxfCode::YCoordinate1,
            31 => DxfCode::ZCoordinate1,
            12 => DxfCode::XCoordinate2,
            22 => DxfCode::YCoordinate2,
            32 => DxfCode::ZCoordinate2,
            13 => DxfCode::XCoordinate3,
            23 => DxfCode::YCoordinate3,
            33 => DxfCode::ZCoordinate3,
            14 => DxfCode::XCoordinate4,
            24 => DxfCode::YCoordinate4,
            34 => DxfCode::ZCoordinate4,
            15 => DxfCode::XCoordinate5,
            25 => DxfCode::YCoordinate5,
            35 => DxfCode::ZCoordinate5,
            16 => DxfCode::XCoordinate6,
            26 => DxfCode::YCoordinate6,
            36 => DxfCode::ZCoordinate6,
            17 => DxfCode::XCoordinate7,
            27 => DxfCode::YCoordinate7,
            37 => DxfCode::ZCoordinate7,
            18 => DxfCode::XCoordinate8,
            28 => DxfCode::YCoordinate8,
            38 => DxfCode::ZCoordinate8,
            39 => DxfCode::Thickness,
            40 => DxfCode::Real40,
            41 => DxfCode::Real41,
            42 => DxfCode::Real42,
            43 => DxfCode::Real43,
            44 => DxfCode::Real44,
            45 => DxfCode::Real45,
            46 => DxfCode::Real46,
            47 => DxfCode::Real47,
            48 => DxfCode::Real48,
            49 => DxfCode::Real49,
            50 => DxfCode::Angle50,
            51 => DxfCode::Angle51,
            52 => DxfCode::Angle52,
            53 => DxfCode::Angle53,
            54 => DxfCode::Angle54,
            55 => DxfCode::Angle55,
            56 => DxfCode::Angle56,
            57 => DxfCode::Angle57,
            58 => DxfCode::Angle58,
            60 => DxfCode::Visibility,
            61 => DxfCode::ParameterSpaceCurveType,
            62 => DxfCode::Color,
            66 => DxfCode::EntitiesFollow,
            67 => DxfCode::ModelSpace,
            68 => DxfCode::ViewportStatus,
            69 => DxfCode::ViewportId,
            70 => DxfCode::Int70,
            71 => DxfCode::Int71,
            72 => DxfCode::Int72,
            73 => DxfCode::Int73,
            74 => DxfCode::Int74,
            75 => DxfCode::Int75,
            76 => DxfCode::Int76,
            77 => DxfCode::Int77,
            78 => DxfCode::Int78,
            79 => DxfCode::Int79,
            90 => DxfCode::Int90,
            91 => DxfCode::Int91,
            92 => DxfCode::Int92,
            93 => DxfCode::Int93,
            94 => DxfCode::Int94,
            95 => DxfCode::Int95,
            96 => DxfCode::Int96,
            97 => DxfCode::Int97,
            98 => DxfCode::Int98,
            99 => DxfCode::Int99,
            100 => DxfCode::SubclassMarker,
            101 => DxfCode::ControlString,
            110 => DxfCode::UcsOriginX,
            120 => DxfCode::UcsOriginY,
            130 => DxfCode::UcsOriginZ,
            111 => DxfCode::UcsXAxisX,
            121 => DxfCode::UcsXAxisY,
            131 => DxfCode::UcsXAxisZ,
            112 => DxfCode::UcsYAxisX,
            122 => DxfCode::UcsYAxisY,
            132 => DxfCode::UcsYAxisZ,
            140 => DxfCode::Real140,
            141 => DxfCode::Real141,
            142 => DxfCode::Real142,
            143 => DxfCode::Real143,
            144 => DxfCode::Real144,
            145 => DxfCode::Real145,
            146 => DxfCode::Real146,
            147 => DxfCode::Real147,
            148 => DxfCode::Real148,
            149 => DxfCode::Real149,
            160 => DxfCode::Int160,
            161 => DxfCode::Int161,
            162 => DxfCode::Int162,
            163 => DxfCode::Int163,
            164 => DxfCode::Int164,
            165 => DxfCode::Int165,
            166 => DxfCode::Int166,
            167 => DxfCode::Int167,
            168 => DxfCode::Int168,
            169 => DxfCode::Int169,
            170 => DxfCode::Int170,
            171 => DxfCode::Int171,
            172 => DxfCode::Int172,
            173 => DxfCode::Int173,
            174 => DxfCode::Int174,
            175 => DxfCode::Int175,
            176 => DxfCode::Int176,
            177 => DxfCode::Int177,
            178 => DxfCode::Int178,
            179 => DxfCode::Int179,
            210 => DxfCode::ExtrusionX,
            220 => DxfCode::ExtrusionY,
            230 => DxfCode::ExtrusionZ,
            270 => DxfCode::Int270,
            271 => DxfCode::Int271,
            272 => DxfCode::Int272,
            273 => DxfCode::Int273,
            274 => DxfCode::Int274,
            275 => DxfCode::Int275,
            276 => DxfCode::Int276,
            277 => DxfCode::Int277,
            278 => DxfCode::Int278,
            279 => DxfCode::Int279,
            280 => DxfCode::Int280,
            281 => DxfCode::Int281,
            282 => DxfCode::Int282,
            283 => DxfCode::Int283,
            284 => DxfCode::Int284,
            285 => DxfCode::Int285,
            286 => DxfCode::Int286,
            287 => DxfCode::Int287,
            288 => DxfCode::Int288,
            289 => DxfCode::Int289,
            290 => DxfCode::Bool290,
            291 => DxfCode::Bool291,
            292 => DxfCode::Bool292,
            293 => DxfCode::Bool293,
            294 => DxfCode::Bool294,
            295 => DxfCode::Bool295,
            296 => DxfCode::Bool296,
            297 => DxfCode::Bool297,
            298 => DxfCode::Bool298,
            299 => DxfCode::Bool299,
            300 => DxfCode::Text300,
            301 => DxfCode::Text301,
            302 => DxfCode::Text302,
            303 => DxfCode::Text303,
            304 => DxfCode::Text304,
            305 => DxfCode::Text305,
            306 => DxfCode::Text306,
            307 => DxfCode::Text307,
            308 => DxfCode::Text308,
            309 => DxfCode::Text309,
            310 => DxfCode::BinaryData310,
            311 => DxfCode::BinaryData311,
            312 => DxfCode::BinaryData312,
            313 => DxfCode::BinaryData313,
            314 => DxfCode::BinaryData314,
            315 => DxfCode::BinaryData315,
            316 => DxfCode::BinaryData316,
            317 => DxfCode::BinaryData317,
            318 => DxfCode::BinaryData318,
            319 => DxfCode::BinaryData319,
            320 => DxfCode::Handle320,
            321 => DxfCode::Handle321,
            322 => DxfCode::Handle322,
            323 => DxfCode::Handle323,
            324 => DxfCode::Handle324,
            325 => DxfCode::Handle325,
            326 => DxfCode::Handle326,
            327 => DxfCode::Handle327,
            328 => DxfCode::Handle328,
            329 => DxfCode::Handle329,
            330 => DxfCode::SoftPointerId330,
            331 => DxfCode::SoftPointerId331,
            332 => DxfCode::SoftPointerId332,
            333 => DxfCode::SoftPointerId333,
            334 => DxfCode::SoftPointerId334,
            335 => DxfCode::SoftPointerId335,
            336 => DxfCode::SoftPointerId336,
            337 => DxfCode::SoftPointerId337,
            338 => DxfCode::SoftPointerId338,
            339 => DxfCode::SoftPointerId339,
            340 => DxfCode::HardPointerId340,
            341 => DxfCode::HardPointerId341,
            342 => DxfCode::HardPointerId342,
            343 => DxfCode::HardPointerId343,
            344 => DxfCode::HardPointerId344,
            345 => DxfCode::HardPointerId345,
            346 => DxfCode::HardPointerId346,
            347 => DxfCode::HardPointerId347,
            348 => DxfCode::HardPointerId348,
            349 => DxfCode::HardPointerId349,
            350 => DxfCode::SoftOwnerId350,
            351 => DxfCode::SoftOwnerId351,
            352 => DxfCode::SoftOwnerId352,
            353 => DxfCode::SoftOwnerId353,
            354 => DxfCode::SoftOwnerId354,
            355 => DxfCode::SoftOwnerId355,
            356 => DxfCode::SoftOwnerId356,
            357 => DxfCode::SoftOwnerId357,
            358 => DxfCode::SoftOwnerId358,
            359 => DxfCode::SoftOwnerId359,
            360 => DxfCode::HardOwnerId360,
            361 => DxfCode::HardOwnerId361,
            362 => DxfCode::HardOwnerId362,
            363 => DxfCode::HardOwnerId363,
            364 => DxfCode::HardOwnerId364,
            365 => DxfCode::HardOwnerId365,
            366 => DxfCode::HardOwnerId366,
            367 => DxfCode::HardOwnerId367,
            368 => DxfCode::HardOwnerId368,
            369 => DxfCode::HardOwnerId369,
            370 => DxfCode::Lineweight,
            380 => DxfCode::PlotStyleNameType,
            390 => DxfCode::PlotStyleNameId,
            400 => DxfCode::Int400,
            401 => DxfCode::Int401,
            402 => DxfCode::Int402,
            403 => DxfCode::Int403,
            404 => DxfCode::Int404,
            405 => DxfCode::Int405,
            406 => DxfCode::Int406,
            407 => DxfCode::Int407,
            408 => DxfCode::Int408,
            409 => DxfCode::Int409,
            410 => DxfCode::LayoutName,
            420 => DxfCode::TrueColor,
            430 => DxfCode::ColorName,
            440 => DxfCode::Transparency,
            450 => DxfCode::Int450,
            451 => DxfCode::Int451,
            452 => DxfCode::Int452,
            453 => DxfCode::Int453,
            454 => DxfCode::Int454,
            455 => DxfCode::Int455,
            456 => DxfCode::Int456,
            457 => DxfCode::Int457,
            458 => DxfCode::Int458,
            459 => DxfCode::Int459,
            460 => DxfCode::Real460,
            461 => DxfCode::Real461,
            462 => DxfCode::Real462,
            463 => DxfCode::Real463,
            464 => DxfCode::Real464,
            465 => DxfCode::Real465,
            466 => DxfCode::Real466,
            467 => DxfCode::Real467,
            468 => DxfCode::Real468,
            469 => DxfCode::Real469,
            470 => DxfCode::Text470,
            471 => DxfCode::Text471,
            472 => DxfCode::Text472,
            473 => DxfCode::Text473,
            474 => DxfCode::Text474,
            475 => DxfCode::Text475,
            476 => DxfCode::Text476,
            477 => DxfCode::Text477,
            478 => DxfCode::Text478,
            479 => DxfCode::Text479,
            480 => DxfCode::Handle480,
            481 => DxfCode::Handle481,
            999 => DxfCode::Comment,
            1000 => DxfCode::XDataString,
            1001 => DxfCode::XDataAppName,
            1002 => DxfCode::XDataControlString,
            1003 => DxfCode::XDataLayerName,
            1004 => DxfCode::XDataBinaryChunk,
            1005 => DxfCode::XDataHandle,
            1010 => DxfCode::XDataXCoordinate,
            1020 => DxfCode::XDataYCoordinate,
            1030 => DxfCode::XDataZCoordinate,
            1011 => DxfCode::XDataWorldXCoordinate,
            1021 => DxfCode::XDataWorldYCoordinate,
            1031 => DxfCode::XDataWorldZCoordinate,
            1012 => DxfCode::XDataWorldXDisplacement,
            1022 => DxfCode::XDataWorldYDisplacement,
            1032 => DxfCode::XDataWorldZCoordinate,
            1013 => DxfCode::XDataWorldXDirection,
            1023 => DxfCode::XDataWorldYDirection,
            1033 => DxfCode::XDataWorldZDirection,
            1040 => DxfCode::XDataReal,
            1041 => DxfCode::XDataDistance,
            1042 => DxfCode::XDataScaleFactor,
            1070 => DxfCode::XDataInteger16,
            1071 => DxfCode::XDataInteger32,
            _ => DxfCode::Invalid,
        }
    }

    /// Convert DxfCode to i32
    pub fn to_i32(self) -> i32 {
        self as i32
    }
}
