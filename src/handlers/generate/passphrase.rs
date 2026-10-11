use rand::{rng, RngExt};

use crate::utils::output::get_formatted_json_string;

const WORDS: &[&str] = &[
    "amber", "anchor", "apex", "apple", "arch", "arrow", "atom", "aurora", "autumn", "bamboo",
    "beacon", "berry", "bird", "blaze", "bloom", "breeze", "brick", "brook", "cactus", "canyon",
    "carbon", "cedar", "cherry", "cloud", "cobalt", "comet", "coral", "cosmos", "crystal", "dawn",
    "delta", "desert", "dune", "eagle", "earth", "echo", "ember", "falcon", "field", "flame",
    "flora", "forest", "frost", "galaxy", "garden", "glacier", "glow", "granite", "grove",
    "harbor", "haze", "helix", "honey", "horizon", "indigo", "iris", "island", "ivory", "jade",
    "jungle", "keystone", "lagoon", "lantern", "leaf", "lemon", "light", "lily", "lotus", "lumen",
    "maple", "marble", "meadow", "meteor", "midnight", "mint", "mist", "monsoon", "moon", "moss",
    "mountain", "nebula", "nectar", "nova", "oasis", "ocean", "olive", "onyx", "opal", "orchid",
    "palm", "pearl", "pepper", "phoenix", "pine", "planet", "plume", "prairie", "quartz", "rain",
    "raven", "reef", "river", "rose", "saffron", "sage", "sand", "sapphire", "scarlet", "shadow",
    "shore", "silver", "sky", "snow", "solar", "spring", "stone", "storm", "summit", "sunset",
    "thunder", "tiger", "timber", "topaz", "tulip", "valley", "velvet", "violet", "water", "wave",
    "willow", "wind", "winter", "wood", "zenith",
];

/// `words` random words from `WORDS` joined by `separator`, e.g.
/// "amber-river-storm". Also used to name agent worktrees.
pub fn generate_passphrase(words: u8, separator: &str) -> String {
    let mut rng = rng();
    (0..words)
        .map(|_| WORDS[rng.random_range(0..WORDS.len())])
        .collect::<Vec<_>>()
        .join(separator)
}

pub fn handle_generate_passphrase(
    words: u8,
    separator: String,
    json_format: bool,
    uppercase: bool,
) {
    let passphrase = generate_passphrase(words, &separator);
    let output = if uppercase {
        passphrase.to_uppercase()
    } else {
        passphrase
    };

    if json_format {
        let json = serde_json::json!({ "value": output });
        let json_pretty = get_formatted_json_string(&json, true).unwrap();
        println!("{}", json_pretty);
    } else {
        println!("{}", output);
    }
}
