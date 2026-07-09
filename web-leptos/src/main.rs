mod core;
mod persist;

use leptos::prelude::*;
use shared::Event;
use shared::schema::{ReadinessInput, ReadinessSignal};

fn readiness(signal: ReadinessSignal, value: f64) -> Event {
    Event::SubmitReadiness(ReadinessInput {
        signal,
        value,
        observed_at: 0,
    })
}

#[component]
fn RootComponent() -> impl IntoView {
    let core = core::new();
    let (view, render) = signal(core.view());

    // Replay any persisted event stream before first paint.
    let restored = persist::load();
    for ev in &restored {
        core::update(&core, ev.clone(), render);
    }
    let log = RwSignal::new(restored);

    // Apply an event to the core, then append it to the persisted log.
    // Plain `Clone` closure (CSR core is `Rc`, not `Send`), cloned per handler.
    let dispatch = move |ev: Event| {
        core::update(&core, ev.clone(), render);
        log.update(|l| l.push(ev));
        persist::save(&log.get_untracked());
    };

    let exercise = RwSignal::new(String::from("Back squat"));
    let weight = RwSignal::new(String::from("100"));
    let reps = RwSignal::new(String::from("5"));
    let rpe = RwSignal::new(String::from("8"));

    let log_set = {
        let dispatch = dispatch.clone();
        move |_| {
            let ex = exercise.get();
            let w = weight.get().parse::<f64>().unwrap_or(0.0);
            let r = reps.get().parse::<u32>().unwrap_or(0);
            let e = rpe.get().parse::<f64>().unwrap_or(0.0);
            if !ex.is_empty() && w > 0.0 && r > 0 {
                dispatch(Event::LogSet {
                    exercise: ex,
                    weight_kg: w,
                    reps: r,
                    rpe: e,
                });
            }
        }
    };

    let dist = RwSignal::new(String::from("10"));
    let dur = RwSignal::new(String::from("50"));
    let hr = RwSignal::new(String::from("70"));
    let longest = RwSignal::new(String::from("12"));

    let log_run = {
        let dispatch = dispatch.clone();
        move |_| {
            let d = dist.get().parse::<f64>().unwrap_or(0.0);
            let t = dur.get().parse::<f64>().unwrap_or(0.0);
            let h = hr.get().parse::<f64>().unwrap_or(0.0);
            let l = longest.get().parse::<f64>().unwrap_or(0.0);
            if d > 0.0 && t > 0.0 {
                dispatch(Event::LogRun {
                    distance_km: d,
                    duration_min: t,
                    hr_pct_max: h,
                    longest_recent_km: l,
                });
            }
        }
    };

    // One cloned dispatcher per inline button handler.
    let d_pain = dispatch.clone();
    let d_rpe_hi = dispatch.clone();
    let d_rpe_lo = dispatch.clone();
    let d_reds = dispatch.clone();
    let d_cardiac = dispatch.clone();
    let d_bone = dispatch.clone();
    let d_clear_r = dispatch.clone();
    let d_clear_s = dispatch.clone();
    let d_clear_run = dispatch.clone();

    view! {
        <section class="box container m-5">
            <h1 class="title is-4">"Readiness → Autoregulation"</h1>

            <p class="is-size-6 mb-2">
                {move || match view.get().safety_tier {
                    Some(t) => format!("Safety tier: {t}"),
                    None => "Safety tier: all clear".to_string(),
                }}
            </p>

            {move || view.get().train_blocked.then(|| view! {
                <div class="notification is-danger">"Training blocked - do not train."</div>
            })}

            <div class="buttons">
                <button class="button is-danger"
                    on:click=move |_| d_pain(readiness(ReadinessSignal::Pain, 1.0))
                >{"Log pain"}</button>
                <button class="button is-warning"
                    on:click=move |_| d_rpe_hi(readiness(ReadinessSignal::Rpe, 2.0))
                >{"Log high RPE"}</button>
                <button class="button is-success"
                    on:click=move |_| d_rpe_lo(readiness(ReadinessSignal::Rpe, 0.0))
                >{"Log easy session"}</button>
                <button class="button"
                    on:click=move |_| d_clear_r(Event::ClearReadiness)
                >{"Clear"}</button>
            </div>

            <p class="is-size-7 has-text-grey mb-1">"Medical red flags → stop + refer:"</p>
            <div class="buttons">
                <button class="button is-danger is-outlined"
                    on:click=move |_| d_cardiac(readiness(ReadinessSignal::CardiacRedFlag, 1.0))
                >{"Cardiac symptom"}</button>
                <button class="button is-danger is-outlined"
                    on:click=move |_| d_bone(readiness(ReadinessSignal::BoneStress, 1.0))
                >{"Bone stress"}</button>
                <button class="button is-danger is-outlined"
                    on:click=move |_| d_reds(readiness(ReadinessSignal::RedS, 1.0))
                >{"RED-S / low energy"}</button>
            </div>

            <p class="is-size-7 mb-2">
                {move || format!("Inputs logged: {}", view.get().input_count)}
            </p>

            <ul>
                {move || view.get().adjustments.into_iter().map(|a| view! {
                    <li class="box p-3 mb-2">
                        <strong>{a.summary}</strong>
                        <span class="tag is-light ml-2">{a.grade}</span>
                        <span class="tag is-info is-light ml-1">
                            {format!("conf {:.2}", a.confidence)}
                        </span>
                        <p class="is-size-7 has-text-grey">{a.citation}</p>
                    </li>
                }).collect_view()}
            </ul>

            <hr/>
            <h2 class="title is-5">"Log a lift set"</h2>
            <div class="field is-grouped is-grouped-multiline">
                <input class="input mr-2" style="width:10rem" placeholder="exercise"
                    prop:value=move || exercise.get()
                    on:input=move |ev| exercise.set(event_target_value(&ev)) />
                <input class="input mr-2" style="width:6rem" type="number" placeholder="kg"
                    prop:value=move || weight.get()
                    on:input=move |ev| weight.set(event_target_value(&ev)) />
                <input class="input mr-2" style="width:5rem" type="number" placeholder="reps"
                    prop:value=move || reps.get()
                    on:input=move |ev| reps.set(event_target_value(&ev)) />
                <input class="input mr-2" style="width:5rem" type="number" step="0.5" placeholder="RPE"
                    prop:value=move || rpe.get()
                    on:input=move |ev| rpe.set(event_target_value(&ev)) />
                <button class="button is-primary" on:click=log_set>{"Log set"}</button>
                <button class="button ml-2" on:click=move |_| d_clear_s(Event::ClearSets)>
                    {"Clear sets"}
                </button>
            </div>

            <ul>
                {move || view.get().lifts.into_iter().map(|l| view! {
                    <li class="box p-3 mb-2">{l.summary}</li>
                }).collect_view()}
            </ul>

            <hr/>
            <h2 class="title is-5">"Log a run"</h2>
            <div class="field is-grouped is-grouped-multiline">
                <input class="input mr-2" style="width:6rem" type="number" placeholder="km"
                    prop:value=move || dist.get()
                    on:input=move |ev| dist.set(event_target_value(&ev)) />
                <input class="input mr-2" style="width:6rem" type="number" placeholder="min"
                    prop:value=move || dur.get()
                    on:input=move |ev| dur.set(event_target_value(&ev)) />
                <input class="input mr-2" style="width:6rem" type="number" placeholder="%HRmax"
                    prop:value=move || hr.get()
                    on:input=move |ev| hr.set(event_target_value(&ev)) />
                <input class="input mr-2" style="width:7rem" type="number" placeholder="longest km"
                    prop:value=move || longest.get()
                    on:input=move |ev| longest.set(event_target_value(&ev)) />
                <button class="button is-primary" on:click=log_run>{"Log run"}</button>
                <button class="button ml-2" on:click=move |_| d_clear_run(Event::ClearRuns)>
                    {"Clear runs"}
                </button>
            </div>

            <ul>
                {move || view.get().runs.into_iter().map(|r| view! {
                    <li class="box p-3 mb-2">
                        <span class=move || if r.spike_flag { "has-text-danger" } else { "" }>
                            {r.summary}
                        </span>
                    </li>
                }).collect_view()}
            </ul>
        </section>
    }
}

fn main() {
    leptos::mount::mount_to_body(|| {
        view! { <RootComponent /> }
    });
}
