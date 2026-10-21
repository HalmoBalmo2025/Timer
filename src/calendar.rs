//! Which dates make up the displayed week and month.
//!
//! The functions return `None` only for dates at the very edge of chrono's
//! calendar, hundreds of thousands of years away.

use chrono::{Datelike, Days, NaiveDate, Weekday};
use std::iter;

/// Monday to Sunday of the week containing `day`.
pub(crate) fn week_dates(day: NaiveDate) -> Option<Vec<NaiveDate>> {
    let (monday, sunday) = day.week(Weekday::Mon).checked_days()?.into_inner();
    Some(dates_from_to(monday, sunday))
}

/// Every date of the month that `day`'s week is booked to.
///
/// Months are made of whole weeks: a week belongs to the month its Sunday
/// falls in. The week ending on a month's last Sunday closes that month, and
/// a week that runs into the next month, be it only with its Sunday, already
/// counts towards the next one. So a month is four or five Mon–Sun weeks,
/// and it may start in the last days of the previous calendar month.
pub(crate) fn month_dates(day: NaiveDate) -> Option<Vec<NaiveDate>> {
    let this_monday = day.week(Weekday::Mon).checked_first_day()?;
    let month = month_of_week(this_monday)?;
    let in_month = |monday: &NaiveDate| month_of_week(*monday) == Some(month);
    let one_week = Days::new(7);
    let first_monday = iter::successors(Some(this_monday), |monday| {
        monday.checked_sub_days(one_week)
    })
    .take_while(in_month)
    .last()?;
    let last_monday = iter::successors(Some(this_monday), |monday| {
        monday.checked_add_days(one_week)
    })
    .take_while(in_month)
    .last()?;
    let last_sunday = last_monday.week(Weekday::Mon).checked_last_day()?;
    Some(dates_from_to(first_monday, last_sunday))
}

/// The month a week is booked to, as its first day: the month of the
/// Sunday that ends the week starting on `monday`.
fn month_of_week(monday: NaiveDate) -> Option<NaiveDate> {
    monday.week(Weekday::Mon).checked_last_day()?.with_day(1)
}

fn dates_from_to(first: NaiveDate, last: NaiveDate) -> Vec<NaiveDate> {
    first.iter_days().take_while(|date| *date <= last).collect()
}

pub(crate) const fn weekday_label(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "Mon",
        Weekday::Tue => "Tue",
        Weekday::Wed => "Wed",
        Weekday::Thu => "Thu",
        Weekday::Fri => "Fri",
        Weekday::Sat => "Sat",
        Weekday::Sun => "Sun",
    }
}
