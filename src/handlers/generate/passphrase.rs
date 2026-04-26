use rand::{rng, Rng};

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

pub fn handle_generate_passphrase(
    words: u8,
    separator: String,
    json_format: bool,
    uppercase: bool,
) {
    let mut rng = rng();
    let mut selected_words: Vec<&str> = Vec::with_capacity(words as usize);

    for _ in 0..words {
        let idx = rng.random_range(0..WORDS.len());
        selected_words.push(WORDS[idx]);
    }

    let passphrase = selected_words.join(&separator);
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
