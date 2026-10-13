use pretty_assertions::assert_eq;

use super::ParsedSkillFrontmatter;
use super::parse_skill_frontmatter_metadata;

#[test]
fn yaml_replacement_preserves_skill_scalar_spellings_and_empty_metadata() {
    for scalar in [
        "true", "TRUE", "tRuE", "NO", "null", "nUlL", "42", "042", "0x2A", "0o52",
        "1_000", "-0.0", ".nan", ".inf", "1e999", "18446744073709551616",
    ] {
        let yaml_scalar = if scalar == "null" { "!custom null" } else { scalar };
        let parsed = parse_skill_frontmatter_metadata(
            &format!("---\ndescription: {yaml_scalar}\nmetadata:\n---\n"),
            || "fallback".to_string(),
        )
        .expect("legacy scalar spelling remains a string");
        assert_eq!(
            parsed,
            ParsedSkillFrontmatter {
                name: "fallback".to_string(),
                description: scalar.to_string(),
                short_description: None,
            },
            "{yaml_scalar}"
        );
    }
}

#[test]
fn yaml_replacement_preserves_required_object_and_duplicate_field_validation() {
    for frontmatter in [
        "description: text\nmetadata: null",
        "description: first\ndescription: second",
        "description: text\nmetadata: [short]",
    ] {
        assert!(
            parse_skill_frontmatter_metadata(&format!("---\n{frontmatter}\n---\n"), || {
                "fallback".to_string()
            })
            .is_err(),
            "{frontmatter}"
        );
    }
    let parsed = parse_skill_frontmatter_metadata(
        "---\ndescription: text\nunknown: first\nunknown: second\n---\n",
        || "fallback".to_string(),
    )
    .expect("unknown fields remain ignored");
    assert_eq!(
        parsed,
        ParsedSkillFrontmatter {
            name: "fallback".to_string(),
            description: "text".to_string(),
            short_description: None,
        }
    );
}

#[test]
fn parses_repairs_and_sanitizes_frontmatter() {
    let parsed = parse_skill_frontmatter_metadata(
        "---\nname:  deploy  service\ndescription: Build for AWS: ECS\nmetadata:\n  short-description:  Deploy   safely\n---\n",
        || "fallback".to_string(),
    )
    .expect("valid frontmatter");

    assert_eq!(
        parsed,
        ParsedSkillFrontmatter {
            name: "deploy service".to_string(),
            description: "Build for AWS: ECS".to_string(),
            short_description: Some("Deploy safely".to_string()),
        }
    );
}

#[test]
fn uses_default_name_and_requires_description() {
    let parsed = parse_skill_frontmatter_metadata("---\ndescription: Demo skill\n---\n", || {
        "demo".to_string()
    })
    .expect("valid frontmatter");
    assert_eq!(
        parsed,
        ParsedSkillFrontmatter {
            name: "demo".to_string(),
            description: "Demo skill".to_string(),
            short_description: None,
        }
    );

    let error =
        parse_skill_frontmatter_metadata("---\nname: demo\n---\n", || "fallback".to_string())
            .expect_err("description should be required");
    assert_eq!(error.to_string(), "missing field `description`");
}

#[test]
fn repairs_short_descriptions_containing_colons_and_apostrophes() {
    let parsed = parse_skill_frontmatter_metadata(
        "---\nname: short\ndescription: Short skill\nmetadata:\n  short-description: What's included: builds and tests\n---\n",
        || "fallback".to_string(),
    )
    .expect("frontmatter with an unquoted short description should be repaired");

    assert_eq!(
        parsed,
        ParsedSkillFrontmatter {
            name: "short".to_string(),
            description: "Short skill".to_string(),
            short_description: Some("What's included: builds and tests".to_string()),
        }
    );
}

#[test]
fn repairs_unrecognized_frontmatter_fields_that_need_quotes() {
    let parsed = parse_skill_frontmatter_metadata(
        "---\nname: unknown\ndescription: Unknown fields\nargument-hint: <duration: e.g. 7d, 2w>\ntags: [next,@supabase/ssr]\n---\n",
        || "fallback".to_string(),
    )
    .expect("frontmatter with unrecognized fields should be repaired");

    assert_eq!(
        parsed,
        ParsedSkillFrontmatter {
            name: "unknown".to_string(),
            description: "Unknown fields".to_string(),
            short_description: None,
        }
    );
}

#[test]
fn preserves_block_scalar_bodies_while_repairing_other_fields() {
    let parsed = parse_skill_frontmatter_metadata(
        "---\nname: block\ndescription: |-\n  Build for AWS: ECS\nargument-hint: <duration: e.g. 7d>\n---\n",
        || "fallback".to_string(),
    )
    .expect("frontmatter repair should preserve block scalar bodies");

    assert_eq!(
        parsed,
        ParsedSkillFrontmatter {
            name: "block".to_string(),
            description: "Build for AWS: ECS".to_string(),
            short_description: None,
        }
    );
}

#[test]
fn preserves_overlong_descriptions_and_short_descriptions() {
    let description = "💡".repeat(/*n*/ 1_025);
    let short_description = "x".repeat(/*n*/ 1_025);
    let parsed = parse_skill_frontmatter_metadata(
        &format!(
            "---\nname: long\ndescription: {description}\nmetadata:\n  short-description: {short_description}\n---\n"
        ),
        || "fallback".to_string(),
    )
    .expect("descriptions should not be truncated during frontmatter parsing");

    assert_eq!(
        parsed,
        ParsedSkillFrontmatter {
            name: "long".to_string(),
            description,
            short_description: Some(short_description),
        }
    );
}
