
                export function get_initial_hydration_data() {
                    const decoded = atob(window.initial_dioxus_hydration_data);
                    return Uint8Array.from(decoded, (c) => c.charCodeAt(0))
                }
                export function get_initial_hydration_debug_types() {
                    return window.initial_dioxus_hydration_debug_types;
                }
                export function get_initial_hydration_debug_locations() {
                    return window.initial_dioxus_hydration_debug_locations;
                }
            