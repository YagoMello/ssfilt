export type Topology = "low-pass" | "high-pass" | "band-pass";
export type ResponseFamily = "repeated-pole" | "butterworth" | "bessel" | "chebyshev1";
export type InputModel = "linear" | "previous-hold" | "current-hold";
export type InitialMode = "sample0" | "custom";

export interface ResponseConfig {
  topology: Topology;
  response: ResponseFamily;
  order: number;
  cutoff_hz?: number;
  lower_cutoff_hz?: number;
  upper_cutoff_hz?: number;
  ripple_db?: number;
  input_model: InputModel;
}

export interface FilterConfig extends ResponseConfig {
  initial_mode: InitialMode;
  initial_value: number;
  first_interval_seconds?: number;
}

export type NumericSeries = Float64Array | number[];

export interface ResponseResult {
  frequency_hz: NumericSeries;
  gain_db: NumericSeries;
  phase_degrees: NumericSeries;
  group_delay_seconds: NumericSeries;
  step_time_seconds: NumericSeries;
  step_output: NumericSeries;
  summary: string;
}

export interface FilterResult {
  time: NumericSeries;
  input: NumericSeries;
  output: NumericSeries;
  accepted_steps: number;
  rejected_steps: number;
  derivative_evaluations: number;
  equilibrium_shortcuts: number;
}

export interface FrequencyGuideResult {
  time_seconds: NumericSeries;
  frequency_hz: NumericSeries;
  magnitude_db: NumericSeries;
  gap_start_seconds: NumericSeries;
  gap_end_seconds: NumericSeries;
  median_interval_seconds: number;
  maximum_interval_seconds: number;
  display_floor_db: number;
  summary: string;
}

export interface SsfiltWasmModule {
  default: () => Promise<unknown>;
  response_data: (config: ResponseConfig) => ResponseResult | Promise<ResponseResult>;
  filter_samples: (
    config: FilterConfig,
    times: Float64Array,
    values: Float64Array,
  ) => FilterResult | Promise<FilterResult>;
  frequency_guide: (
    config: ResponseConfig,
    times: Float64Array,
    values: Float64Array,
  ) => FrequencyGuideResult | Promise<FrequencyGuideResult>;
}

let adapterPromise: Promise<SsfiltWasmModule> | undefined;

export function loadAdapter(): Promise<SsfiltWasmModule> {
  adapterPromise ??= (async () => {
    const appBase = new URL(import.meta.env.BASE_URL, window.location.href);
    const moduleUrl = new URL("wasm/pkg/ssfilt_web_wasm.js", appBase);
    const loaded = (await import(/* @vite-ignore */ moduleUrl.href)) as SsfiltWasmModule;
    await loaded.default();
    return loaded;
  })();

  return adapterPromise;
}
