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
        </section>
    }
}

fn main() {
    leptos::mount::mount_to_body(|| {
        view! { <RootComponent /> }
    });
}
