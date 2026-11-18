# Strategy: renewals for the clinic scheduling product

## Diagnosis
- D1: Renewals fall because clinics that never set up reminders see no drop in missed appointments.
  Falsifier: clinics with reminders switched on renew at the same rate as clinics without.
- D2: Setup takes a trained administrator, and most small clinics have none.
  Falsifier: median setup time is under one hour for clinics with no administrator.

## Guiding policies
- P1 (D1): Every new clinic has reminders running before its first invoice.
- P2 (D2): Setup must be possible for a receptionist with no training.
- P3: Be the most loved brand in dental software.

## Actions
- A1 (P1): Switch reminders on by default at sign-up.
  Owner: onboarding team
  Measure: share of new clinics with reminders on in week one
  Stop doing: optional reminder setup in month two
- A2 (P2): Replace the setup manual with a guided setup of three steps.
  Owner: product design
  Measure: median setup time for a clinic with no administrator
- A3 (P1, P2): Call every clinic that has no reminders after 14 days.
  Owner: customer success
  Measure: share of called clinics with reminders on a week later
- A4 (P3): Redesign the logo.
  Owner: marketing
  Measure: brand survey score

## Forecasts
- Forecast (2027-06-30): the renewal rate among clinics onboarded after the change is at least 85 percent.
  Tripwire: fewer than half of new clinics have reminders on in week one.
