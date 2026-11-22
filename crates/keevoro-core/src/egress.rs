//! What reading files out of cloud storage is likely to cost, said before a
//! backup that would do it starts.
//!
//! Most cloud storage charges for data taken out of it ("egress"), and a
//! person moving an old backup somewhere cheaper should know before they
//! press Back up now, not from their bill. The prices are kept here in one
//! table, with the day they were last checked against each provider's own
//! price page; every sentence says "about", and a test fails a year after
//! that day so that somebody checks them again. Pure.

/// The day the prices below were last checked, as (year, month, day).
pub const CHECKED: (i64, u32, u32) = (2026, 11, 21);

/// Pounds to the dollar, as the prices were checked. Prices are in dollars;
/// people are told them in pounds.
pub const POUNDS_PER_DOLLAR: f64 = 0.79;

/// What each provider charges to read data out of it to the internet, in
/// dollars a gigabyte (2^30 bytes, as they bill), at its usual rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Price {
    /// A charge for each gigabyte.
    PerGigabyte(f64),
    /// No charge for downloading.
    Free,
    /// Free up to a limit set by what is kept there, then a charge.
    FreeUpTo { times_stored: u32, then: f64 },
    /// Not known.
    Unknown,
}

/// Amazon S3: about 9 US cents a gigabyte out to the internet, after the
/// first 100 GB a month across the whole account.
pub const AMAZON_S3: Price = Price::PerGigabyte(0.09);
/// What Amazon gives each account free each month, in gigabytes.
pub const AMAZON_FREE_EACH_MONTH: u64 = 100;
/// Cloudflare R2: no charge for downloading.
pub const CLOUDFLARE_R2: Price = Price::Free;
/// Backblaze B2: free up to three times what is stored, then 1 US cent a
/// gigabyte.
pub const BACKBLAZE_B2: Price = Price::FreeUpTo {
    times_stored: 3,
    then: 0.01,
};
/// Wasabi: no charge for downloading, within fair use.
pub const WASABI: Price = Price::Free;

/// Which provider, and its price, for a kind of storage and its place (the
/// account, region or address). Storage at another address is known by
/// its host where it is one of the common ones.
pub fn provider(kind: &str, place: &str) -> (&'static str, Price) {
    let host = place.to_lowercase();
    match kind {
        "s3" => ("Amazon S3", AMAZON_S3),
        "r2" => ("Cloudflare R2", CLOUDFLARE_R2),
        _ if host.contains("backblazeb2.com") => ("Backblaze B2", BACKBLAZE_B2),
        _ if host.contains("wasabisys.com") => ("Wasabi", WASABI),
        _ if host.contains("r2.cloudflarestorage.com") => ("Cloudflare R2", CLOUDFLARE_R2),
        _ if host.contains("amazonaws.com") => ("Amazon S3", AMAZON_S3),
        _ => ("this storage", Price::Unknown),
    }
}

const GIGABYTE: f64 = 1024.0 * 1024.0 * 1024.0;

/// About how much, in pounds, `bytes` at so much a gigabyte comes to, said
/// as a person would: "under £1", "about £9", "about £1,200".
fn pounds(bytes: u64, dollars_per_gigabyte: f64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let pounds = bytes as f64 / GIGABYTE * dollars_per_gigabyte * POUNDS_PER_DOLLAR;
    if pounds < 1.0 {
        "under £1".to_string()
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let whole = pounds.round() as u64;
        format!(
            "about £{}",
            crate::words::thousands(usize::try_from(whole).unwrap_or(usize::MAX))
        )
    }
}

/// About what reading `bytes` out of a provider costs, in one or two
/// sentences, each with "about" in it. `size` is the amount as the screen
/// says it ("120 GB"), so the two never differ.
pub fn cost_said(kind: &str, place: &str, bytes: u64, size: &str) -> String {
    let (name, price) = provider(kind, place);
    match price {
        Price::PerGigabyte(dollars) => {
            let mut said = format!(
                "Reading {size} out of {name} costs {} at {}’s usual price; Cloudflare R2 charges nothing for this.",
                pounds(bytes, dollars),
                name.split(' ').next().unwrap_or(name)
            );
            if name == "Amazon S3" {
                said.push_str(&format!(
                    " Amazon lets each account download about {AMAZON_FREE_EACH_MONTH} GB a month free, which may \
                     make it less."
                ));
            }
            said
        }
        Price::Free => {
            format!("Reading {size} out of {name} costs about nothing: {name} doesn’t charge for downloading.")
        }
        Price::FreeUpTo { times_stored, then } => format!(
            "Reading {size} out of {name} costs about nothing while you download less than {} what you keep \
             there each month, and {} beyond that.",
            match times_stored {
                2 => "twice".to_string(),
                3 => "three times".to_string(),
                n => format!("{n} times"),
            },
            pounds(bytes, then)
        ),
        Price::Unknown => format!(
            "Some cloud storage charges about 7p a gigabyte to read files out of it, and Keevoro doesn’t know what \
             {name} charges for {size}, so check before you start."
        ),
    }
}

/// About what reading `bytes` out of a provider costs, in pounds, for a
/// rule to weigh: its usual price, with no free allowance counted (the
/// careful way), and storage whose price is not known at 7p a gigabyte.
pub fn estimated_pounds(kind: &str, place: &str, bytes: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let gigabytes = bytes as f64 / GIGABYTE;
    match provider(kind, place).1 {
        Price::PerGigabyte(dollars) => gigabytes * dollars * POUNDS_PER_DOLLAR,
        Price::Free | Price::FreeUpTo { .. } => 0.0,
        Price::Unknown => gigabytes * 0.07,
    }
}

/// The days from 1 January 1970 to a date, for knowing how old the table
/// is. The usual civil calendar arithmetic.
pub fn days_since_1970(year: i64, month: u32, day: u32) -> i64 {
    let (y, m) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * i64::from(m) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn what_reading_costs_is_weighed_the_careful_way() {
        assert!((estimated_pounds("s3", "eu-west-2", 100 * GB) - 7.11).abs() < 0.01);
        assert!(estimated_pounds("r2", "acc", 100 * GB).abs() < f64::EPSILON);
        assert!((estimated_pounds("endpoint", "https://example.com", 100 * GB) - 7.0).abs() < 0.01);
    }

    #[test]
    fn reading_out_of_amazon_is_said_in_pounds_with_about_and_cloudflare_beside_it() {
        assert_eq!(
            cost_said("s3", "eu-west-2", 120 * GB, "120 GB"),
            "Reading 120 GB out of Amazon S3 costs about £9 at Amazon’s usual price; Cloudflare R2 charges nothing \
             for this. Amazon lets each account download about 100 GB a month free, which may make it less."
        );
        assert!(cost_said("s3", "eu-west-2", 2 * GB, "2 GB").contains("costs under £1"));
        assert!(cost_said("s3", "eu-west-2", 20_000 * GB, "19.5 TB").contains("about £1,422"));
    }

    #[test]
    fn every_provider_is_said_with_about() {
        for (kind, place) in [
            ("r2", "0123"),
            ("s3", "eu-west-2"),
            ("endpoint", "https://s3.eu-central-003.backblazeb2.com"),
            ("endpoint", "https://s3.wasabisys.com"),
            ("endpoint", "https://nas.local:9000"),
        ] {
            let said = cost_said(kind, place, 500 * GB, "500 GB");
            for sentence in said.split(". ").filter(|s| !s.is_empty()) {
                assert!(sentence.contains("about"), "{kind} {place}: {sentence}");
            }
            assert!(!said.contains('\u{2014}'), "{said}");
        }
        assert_eq!(
            cost_said("r2", "0123", 500 * GB, "500 GB"),
            "Reading 500 GB out of Cloudflare R2 costs about nothing: Cloudflare R2 doesn’t charge for downloading."
        );
        assert!(cost_said("endpoint", "https://nas.local:9000", GB, "1 GB").contains("Keevoro doesn’t know"));
        assert_eq!(provider("endpoint", "https://S3.WASABISYS.COM").0, "Wasabi");
    }

    #[test]
    fn days_are_counted_from_1970() {
        assert_eq!(days_since_1970(1970, 1, 1), 0);
        assert_eq!(days_since_1970(2000, 3, 1), 11_017);
        assert_eq!(days_since_1970(2026, 11, 21), 20_778);
    }

    /// Prices change. A year after they were last checked this fails, so
    /// somebody looks at each provider's price page again, puts the table
    /// right, and moves [`CHECKED`] on.
    #[test]
    fn the_table_of_prices_was_checked_within_a_year() {
        let today = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            / 86_400;
        let checked = days_since_1970(CHECKED.0, CHECKED.1, CHECKED.2);
        let age = i64::try_from(today).unwrap() - checked;
        assert!(
            age <= 365,
            "The cloud storage prices in egress.rs were last checked {age} days ago. Check each provider's price \
             page, put the table right, and move CHECKED on."
        );
    }
}
