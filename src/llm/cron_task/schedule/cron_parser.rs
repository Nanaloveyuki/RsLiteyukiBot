use std::collections::HashSet;

use chrono::{DateTime, Datelike, Duration, FixedOffset, Offset, Timelike, Utc};

const SCHEDULER_LOOKAHEAD_MINUTES: i64 = 366 * 24 * 60;

#[derive(Debug, Clone)]
struct ParsedCronFields {
    minute: CronField,
    hour: CronField,
    day_of_month: CronField,
    month: CronField,
    day_of_week: CronField,
}

#[derive(Debug, Clone)]
struct CronField {
    any: bool,
    values: HashSet<u32>,
}

pub(super) fn next_cron_occurrence(
    expression: &str,
    timezone: Option<&str>,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    let fields = parse_cron_expression(expression)?;
    let offset = parse_timezone_offset(timezone)?;
    let mut candidate = after.with_timezone(&offset);
    candidate = candidate
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .ok_or_else(|| "failed to normalize cron candidate".to_string())?
        + Duration::minutes(1);

    for _ in 0..SCHEDULER_LOOKAHEAD_MINUTES {
        if cron_fields_match(&fields, candidate) {
            return Ok(candidate.with_timezone(&Utc));
        }
        candidate += Duration::minutes(1);
    }

    Err(format!(
        "cron expression '{}' has no matching run time within the scheduler lookahead window",
        expression.trim()
    ))
}

fn parse_cron_expression(expression: &str) -> Result<ParsedCronFields, String> {
    let parts = expression
        .split_whitespace()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() != 5 {
        return Err("cron expression must contain exactly five fields".to_string());
    }

    Ok(ParsedCronFields {
        minute: parse_cron_field(parts[0], 0, 59)?,
        hour: parse_cron_field(parts[1], 0, 23)?,
        day_of_month: parse_cron_field(parts[2], 1, 31)?,
        month: parse_cron_field(parts[3], 1, 12)?,
        day_of_week: parse_cron_field(parts[4], 0, 7)?,
    })
}

fn parse_cron_field(source: &str, min: u32, max: u32) -> Result<CronField, String> {
    let trimmed = source.trim();
    if trimmed == "*" {
        return Ok(CronField {
            any: true,
            values: HashSet::new(),
        });
    }

    let mut values = HashSet::new();
    for segment in trimmed.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            return Err(format!("invalid cron field '{}'", source));
        }

        if let Some(step_source) = segment.strip_prefix("*/") {
            let step = parse_cron_number(step_source, min, max)?;
            if step == 0 {
                return Err(format!("invalid cron step '{}'", segment));
            }
            insert_stepped_values(&mut values, min, max, step);
            continue;
        }

        if let Some((range_source, step_source)) = segment.split_once('/') {
            let step = parse_cron_number(step_source, min, max)?;
            if step == 0 {
                return Err(format!("invalid cron step '{}'", segment));
            }
            let (start, end) = parse_cron_range(range_source, min, max)?;
            insert_stepped_values(&mut values, start, end, step);
            continue;
        }

        if segment.contains('-') {
            let (start, end) = parse_cron_range(segment, min, max)?;
            for value in start..=end {
                values.insert(value);
            }
            continue;
        }

        values.insert(parse_cron_number(segment, min, max)?);
    }

    Ok(CronField { any: false, values })
}

fn insert_stepped_values(values: &mut HashSet<u32>, start: u32, end: u32, step: u32) {
    let mut value = start;
    while value <= end {
        values.insert(value);
        value = value.saturating_add(step);
        if value == 0 {
            break;
        }
    }
}

fn parse_cron_range(segment: &str, min: u32, max: u32) -> Result<(u32, u32), String> {
    let (start, end) = segment
        .split_once('-')
        .ok_or_else(|| format!("invalid cron range '{}'", segment))?;
    let start = parse_cron_number(start, min, max)?;
    let end = parse_cron_number(end, min, max)?;
    if start > end {
        return Err(format!("invalid descending cron range '{}'", segment));
    }
    Ok((start, end))
}

fn parse_cron_number(source: &str, min: u32, max: u32) -> Result<u32, String> {
    let value = source
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("invalid cron number '{}'", source.trim()))?;
    if value < min || value > max {
        return Err(format!(
            "cron value '{}' is outside the supported range {}..={}",
            value, min, max
        ));
    }
    Ok(value)
}

fn cron_fields_match(fields: &ParsedCronFields, candidate: DateTime<FixedOffset>) -> bool {
    let minute_matches = field_matches(&fields.minute, candidate.minute());
    let hour_matches = field_matches(&fields.hour, candidate.hour());
    let month_matches = field_matches(&fields.month, candidate.month());
    let day_of_month_matches = field_matches(&fields.day_of_month, candidate.day());
    let weekday = candidate.weekday().num_days_from_sunday();
    let day_of_week_matches = field_matches(&fields.day_of_week, weekday)
        || (weekday == 0 && field_matches(&fields.day_of_week, 7));

    minute_matches
        && hour_matches
        && month_matches
        && day_match(
            day_of_month_matches,
            day_of_week_matches,
            &fields.day_of_month,
            &fields.day_of_week,
        )
}

fn day_match(
    day_of_month_matches: bool,
    day_of_week_matches: bool,
    day_of_month: &CronField,
    day_of_week: &CronField,
) -> bool {
    match (day_of_month.any, day_of_week.any) {
        (true, true) => true,
        (false, true) => day_of_month_matches,
        (true, false) => day_of_week_matches,
        (false, false) => day_of_month_matches || day_of_week_matches,
    }
}

fn field_matches(field: &CronField, value: u32) -> bool {
    field.any || field.values.contains(&value)
}

fn parse_timezone_offset(source: Option<&str>) -> Result<FixedOffset, String> {
    let Some(source) = source.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Utc.fix());
    };
    if source.eq_ignore_ascii_case("utc") || source.eq_ignore_ascii_case("z") {
        return Ok(Utc.fix());
    }

    let source = source.replace("UTC", "").replace("utc", "");
    let source = source.trim().to_string();
    if source.is_empty() {
        return Ok(Utc.fix());
    }

    let sign = if source.starts_with('-') { -1 } else { 1 };
    let numeric = source.trim_start_matches(['+', '-']);
    let (hours, minutes) = if let Some((hours, minutes)) = numeric.split_once(':') {
        (
            hours
                .parse::<i32>()
                .map_err(|_| format!("invalid cron timezone '{}'", source))?,
            minutes
                .parse::<i32>()
                .map_err(|_| format!("invalid cron timezone '{}'", source))?,
        )
    } else if numeric.len() == 4 {
        (
            numeric[..2]
                .parse::<i32>()
                .map_err(|_| format!("invalid cron timezone '{}'", source))?,
            numeric[2..]
                .parse::<i32>()
                .map_err(|_| format!("invalid cron timezone '{}'", source))?,
        )
    } else {
        (
            numeric
                .parse::<i32>()
                .map_err(|_| format!("invalid cron timezone '{}'", source))?,
            0,
        )
    };
    let seconds = sign * (hours * 3600 + minutes * 60);
    FixedOffset::east_opt(seconds)
        .ok_or_else(|| format!("invalid cron timezone '{}'", source.trim()))
}
