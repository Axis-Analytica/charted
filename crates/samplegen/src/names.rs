//! Hand-written word lists. Everything here is invented for the sample, except city
//! names and their countries, which are facts.

pub const FIRST_SYLLABLES: &[&str] = &[
    "Al", "Bri", "Ca", "Da", "El", "Fen", "Ga", "Ha", "Is", "Jo", "Ka", "Li", "Ma", "No", "Or",
    "Pe", "Ri", "Sa", "Te", "Vi", "Wy", "Ze",
];
pub const FIRST_ENDINGS: &[&str] = &[
    "na", "ra", "lo", "an", "en", "ia", "on", "is", "ey", "ah", "ia", "o", "ie", "us", "et",
];
pub const LAST_STARTS: &[&str] = &[
    "Ash", "Birch", "Cole", "Dun", "East", "Fair", "Glen", "Hart", "Iron", "Kings", "Lang",
    "Marsh", "North", "Oak", "Pen", "Quill", "Rock", "Stan", "Thorn", "Wex",
];
pub const LAST_ENDS: &[&str] = &[
    "wood", "ford", "ley", "by", "ton", "well", "more", "field", "worth", "dale", "ridge", "brook",
];

/// (city, country, region)
pub const CITIES: &[(&str, &str, &str)] = &[
    ("Seattle", "United States", "North America"),
    ("Portland", "United States", "North America"),
    ("Denver", "United States", "North America"),
    ("Boulder", "United States", "North America"),
    ("Salt Lake City", "United States", "North America"),
    ("Minneapolis", "United States", "North America"),
    ("Chicago", "United States", "North America"),
    ("Boston", "United States", "North America"),
    ("Burlington", "United States", "North America"),
    ("Asheville", "United States", "North America"),
    ("Austin", "United States", "North America"),
    ("Vancouver", "Canada", "North America"),
    ("Calgary", "Canada", "North America"),
    ("Toronto", "Canada", "North America"),
    ("Montreal", "Canada", "North America"),
    ("Halifax", "Canada", "North America"),
    ("Edinburgh", "United Kingdom", "Europe"),
    ("Manchester", "United Kingdom", "Europe"),
    ("Bristol", "United Kingdom", "Europe"),
    ("London", "United Kingdom", "Europe"),
    ("Dublin", "Ireland", "Europe"),
    ("Oslo", "Norway", "Europe"),
    ("Bergen", "Norway", "Europe"),
    ("Stockholm", "Sweden", "Europe"),
    ("Copenhagen", "Denmark", "Europe"),
    ("Aarhus", "Denmark", "Europe"),
    ("Helsinki", "Finland", "Europe"),
    ("Amsterdam", "Netherlands", "Europe"),
    ("Munich", "Germany", "Europe"),
    ("Berlin", "Germany", "Europe"),
    ("Hamburg", "Germany", "Europe"),
    ("Zurich", "Switzerland", "Europe"),
    ("Innsbruck", "Austria", "Europe"),
    ("Chamonix", "France", "Europe"),
    ("Lyon", "France", "Europe"),
    ("Barcelona", "Spain", "Europe"),
    ("Milan", "Italy", "Europe"),
    ("Sydney", "Australia", "Asia Pacific"),
    ("Melbourne", "Australia", "Asia Pacific"),
    ("Brisbane", "Australia", "Asia Pacific"),
    ("Hobart", "Australia", "Asia Pacific"),
    ("Perth", "Australia", "Asia Pacific"),
    ("Auckland", "New Zealand", "Asia Pacific"),
    ("Wellington", "New Zealand", "Asia Pacific"),
    ("Queenstown", "New Zealand", "Asia Pacific"),
    ("Tokyo", "Japan", "Asia Pacific"),
    ("Sapporo", "Japan", "Asia Pacific"),
    ("Singapore", "Singapore", "Asia Pacific"),
];

/// (category, subcategories). Outdoor-themed so seasonality is believable.
pub const CATEGORIES: &[(&str, &[&str])] = &[
    (
        "Outerwear",
        &[
            "Rain Jackets",
            "Insulated Jackets",
            "Fleece",
            "Vests",
            "Ski Shells",
        ],
    ),
    (
        "Footwear",
        &[
            "Trail Running",
            "Hiking Boots",
            "Approach Shoes",
            "Sandals",
            "Winter Boots",
        ],
    ),
    (
        "Camping",
        &[
            "Tents",
            "Sleeping Bags",
            "Sleeping Pads",
            "Stoves",
            "Camp Furniture",
        ],
    ),
    (
        "Packs",
        &[
            "Daypacks",
            "Backpacking Packs",
            "Travel Bags",
            "Hydration Packs",
        ],
    ),
    (
        "Apparel",
        &[
            "Base Layers",
            "Shirts",
            "Pants",
            "Shorts",
            "Socks",
            "Hats & Gloves",
        ],
    ),
    (
        "Accessories",
        &[
            "Headlamps",
            "Water Bottles",
            "Trekking Poles",
            "Navigation",
            "First Aid",
        ],
    ),
];

pub const BRANDS: &[&str] = &[
    "Harbor & Pine",
    "Northfold",
    "Kestrel Works",
    "Tidewater",
    "Granite Peak",
    "Alder & Co",
    "Loamline",
    "Summitry",
];
pub const COLORS: &[&str] = &[
    "Black", "Slate", "Forest", "Ochre", "Navy", "Rust", "Sand", "Glacier", "Moss", "Charcoal",
];
pub const PRODUCT_WORDS: &[&str] = &[
    "Ridgeline",
    "Cascade",
    "Tundra",
    "Driftwood",
    "Switchback",
    "Basecamp",
    "Fjord",
    "Cirrus",
    "Timberline",
    "Harbor",
    "Lichen",
    "Scree",
    "Boreal",
    "Contour",
    "Alpenglow",
    "Talus",
];
pub const SEGMENTS: &[(&str, f64)] = &[
    ("Consumer", 0.72),
    ("Outdoor Club", 0.18),
    ("Business", 0.10),
];
