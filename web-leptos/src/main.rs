mod core;

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
    let (event, set_event) = signal(Event::ClearReadiness);

    Effect::new(move |_| {
        core::update(&core, event.get(), render);
    });

    let exercise = RwSignal::new(String::from("Back squat"));
    let weight = RwSignal::new(String::from("100"));
    let reps = RwSignal::new(String::from("5"));
    let rpe = RwSignal::new(String::from("8"));

    let log_set = move |_| {
        let ex = exercise.get();
        let w = weight.get().parse::<f64>().unwrap_or(0.0);
        let r = reps.get().parse::<u32>().unwrap_or(0);
        let e = rpe.get().parse::<f64>().unwrap_or(0.0);
        if !ex.is_empty() && w > 0.0 && r > 0 {
            set_event.set(Event::LogSet {
                exercise: ex,
                weight_kg: w,
                reps: r,
                rpe: e,
            });
        }
    };

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
                    on:click=move |_| set_event.set(readiness(ReadinessSignal::Pain, 1.0))
                >{"Log pain"}</button>
                <button class="button is-warning"
                    on:click=move |_| set_event.set(readiness(ReadinessSignal::Rpe, 2.0))
                >{"Log high RPE"}</button>
                <button class="button is-success"
                    on:click=move |_| set_event.set(readiness(ReadinessSignal::Rpe, 0.0))
                >{"Log easy session"}</button>
                <button class="button"
                    on:click=move |_| set_event.set(Event::ClearReadiness)
                >{"Clear"}</button>
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
                <button class="button ml-2" on:click=move |_| set_event.set(Event::ClearSets)>
                    {"Clear sets"}
                </button>
            </div>

            <ul>
                {move || view.get().lifts.into_iter().map(|l| view! {
                    <li class="box p-3 mb-2">{l.summary}</li>
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
