import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import hljs from "highlight.js/lib/core";
import rust from "highlight.js/lib/languages/rust";
import "./style.css";
import {
  loadAdapter,
  type FilterConfig,
  type FilterResult,
  type FrequencyGuideResult,
  type InitialMode,
  type InputModel,
  type NumericSeries,
  type ResponseConfig,
  type ResponseFamily,
  type ResponseResult,
  type SsfiltWasmModule,
  type Topology,
} from "./adapter";
import { MAX_CSV_BYTES, parseCsv, type ParsedCsv } from "./csv";

type ChartKind = "magnitude" | "phase" | "delay" | "step";
type ExampleSignal = "noisy-sine" | "step-drift" | "motion-pulses";

const MAGNITUDE_DISPLAY_FLOOR_DB = -140;

hljs.registerLanguage("rust", rust);

const ui = {
  topology: getElement<HTMLSelectElement>("topology"),
  response: getElement<HTMLSelectElement>("response"),
  order: getElement<HTMLInputElement>("order"),
  cutoff: getElement<HTMLInputElement>("cutoff"),
  lowerCutoff: getElement<HTMLInputElement>("lower-cutoff"),
  upperCutoff: getElement<HTMLInputElement>("upper-cutoff"),
  ripple: getElement<HTMLInputElement>("ripple"),
  inputModel: getElement<HTMLSelectElement>("input-model"),
  singleCutoffField: getElement<HTMLDivElement>("single-cutoff-field"),
  bandCutoffFields: getElement<HTMLDivElement>("band-cutoff-fields"),
  rippleField: getElement<HTMLDivElement>("ripple-field"),
  topologyHint: getElement<HTMLParagraphElement>("topology-hint"),
  inputModelHint: getElement<HTMLParagraphElement>("input-model-hint"),
  validation: getElement<HTMLDivElement>("validation-message"),
  responseTitle: getElement<HTMLHeadingElement>("response-card-title"),
  responseSummary: getElement<HTMLDivElement>("response-summary"),
  responseCard: document.querySelector<HTMLDivElement>(".response-card")!,
  chartHost: getElement<HTMLDivElement>("response-chart"),
  frequencyChartHost: getElement<HTMLDivElement>("response-frequency-chart"),
  stepChartHost: getElement<HTMLDivElement>("response-step-chart"),
  chartPanel: getElement<HTMLDivElement>("response-chart-panel"),
  chartTitle: getElement<HTMLHeadingElement>("chart-title"),
  chartSubtitle: getElement<HTMLParagraphElement>("chart-subtitle"),
  chartReferenceKey: getElement<HTMLDivElement>("chart-reference-key"),
  cutoffReferenceLabel: getElement<HTMLSpanElement>("cutoff-reference-label"),
  magnitudeFloorKey: getElement<HTMLSpanElement>("magnitude-floor-key"),
  responseChartSummary: getElement<HTMLElement>("response-chart-summary"),
  code: getElement<HTMLElement>("rust-code"),
  codeCard: document.querySelector<HTMLElement>(".code-card")!,
  codeToggle: getElement<HTMLButtonElement>("toggle-code"),
  dataFilterSummary: getElement<HTMLElement>("data-filter-summary"),
  dataFilterNote: getElement<HTMLElement>("data-filter-note"),
  initialModes: [...document.querySelectorAll<HTMLInputElement>('input[name="initial-mode"]')],
  initialModeDescription: getElement<HTMLParagraphElement>("initial-mode-description"),
  customInitialFields: getElement<HTMLDivElement>("custom-initial-fields"),
  initialValue: getElement<HTMLInputElement>("initial-value"),
  firstInterval: getElement<HTMLInputElement>("first-interval"),
  csvText: getElement<HTMLTextAreaElement>("csv-text"),
  csvLineNumbers: getElement<HTMLPreElement>("csv-line-numbers"),
  csvFile: getElement<HTMLInputElement>("csv-file"),
  exampleSignal: getElement<HTMLSelectElement>("example-signal"),
  csvStatus: getElement<HTMLDivElement>("csv-status"),
  csvError: getElement<HTMLDivElement>("csv-error"),
  filteredPlaceholder: getElement<HTMLDivElement>("filtered-placeholder"),
  filteredPlaceholderTitle: getElement<HTMLElement>("filtered-placeholder-title"),
  filteredPlaceholderCopy: getElement<HTMLElement>("filtered-placeholder-copy"),
  filteredResult: getElement<HTMLDivElement>("filtered-result"),
  filteredContent: getElement<HTMLDivElement>("filtered-content"),
  downloadCsv: getElement<HTMLButtonElement>("download-csv"),
  filterChart: getElement<HTMLDivElement>("filter-chart"),
  filterChartSummary: getElement<HTMLParagraphElement>("filter-chart-summary"),
  frequencyGuideToggle: getElement<HTMLButtonElement>("toggle-frequency-guide"),
  frequencyGuideContent: getElement<HTMLDivElement>("frequency-guide-content"),
  frequencyGuideCanvas: getElement<HTMLCanvasElement>("frequency-guide-canvas"),
  frequencyGuideStatus: getElement<HTMLParagraphElement>("frequency-guide-status"),
  toast: getElement<HTMLDivElement>("toast"),
};

const chartTabs = [...document.querySelectorAll<HTMLButtonElement>("[role=tab][data-chart]")];
const chartMeta: Record<ChartKind, { title: string; subtitle: string; x: string; y: string }> = {
  magnitude: { title: "Magnitude response", subtitle: "Unclamped gain · visible range ends at −140 dB", x: "Frequency · Hz", y: "Gain · dB" },
  phase: { title: "Phase response", subtitle: "Natural phase shift across frequency", x: "Frequency · Hz", y: "Phase · degrees" },
  delay: { title: "Group delay", subtitle: "Delay across frequency", x: "Frequency · Hz", y: "Group delay · s" },
  step: { title: "Streamed step trace", subtitle: "Canonical current-hold step · bounded time window", x: "Time · s", y: "Output" },
};

let activeChart: ChartKind = "magnitude";
let adapter: SsfiltWasmModule | undefined;
let responseResult: ResponseResult | undefined;
let loadedCsv: ParsedCsv | undefined;
let filterResult: FilterResult | undefined;
let frequencyPlot: uPlot | undefined;
let stepPlot: uPlot | undefined;
let filterPlot: uPlot | undefined;
let frequencyGuideResult: FrequencyGuideResult | undefined;
let frequencyGuideOpen = false;
let frequencyGuideRequestId = 0;
let signalRevision = 0;
let analyzedSignalRevision = -1;
let analyzedInputModel: InputModel | undefined;
let appliedConfig: ResponseConfig | undefined;
let toastTimer = 0;
let responseDebounceTimer = 0;
let responseRequestId = 0;
let filterRequestId = 0;
let resizeFrame = 0;
let responseContainerWidth = 0;
let filterContainerWidth = 0;

function getElement<T extends Element>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`Required page element #${id} is missing.`);
  return element as unknown as T;
}

function selectedInitialMode(): InitialMode {
  return (ui.initialModes.find((input) => input.checked)?.value as InitialMode | undefined) ?? "sample0";
}

function selectInitialMode(mode: InitialMode): void {
  ui.initialModes.forEach((input) => {
    input.checked = input.value === mode;
  });
}

function numericSeries(values: NumericSeries): number[] {
  return Array.from(values, (value) => Number(value));
}

function shortNumber(value: number, digits = 3): string {
  if (!Number.isFinite(value)) return "not available";
  return new Intl.NumberFormat(undefined, { maximumSignificantDigits: digits }).format(value);
}

function niceLinearStep(span: number, targetIntervals: number): number {
  const roughStep = span / Math.max(1, targetIntervals);
  const magnitude = 10 ** Math.floor(Math.log10(roughStep));
  const normalized = roughStep / magnitude;
  const multiplier = normalized <= 1 ? 1 : normalized <= 2 ? 2 : normalized <= 2.5 ? 2.5 : normalized <= 5 ? 5 : 10;
  return multiplier * magnitude;
}

function timeAxisTicks(minimum: number, maximum: number, plotWidth: number): number[] {
  if (!Number.isFinite(minimum) || !Number.isFinite(maximum) || maximum <= minimum) return [];
  const targetIntervals = Math.max(2, Math.min(12, Math.floor(plotWidth / 68)));
  const step = niceLinearStep(maximum - minimum, targetIntervals);
  const tolerance = step * 1e-9;
  const first = Math.ceil((minimum - tolerance) / step) * step;
  const ticks: number[] = [];
  for (let value = first; value <= maximum + tolerance && ticks.length < 64; value += step) {
    ticks.push(Math.abs(value) < tolerance ? 0 : value);
  }
  return ticks;
}

function tickStep(ticks: number[]): number {
  if (ticks.length < 2) return Math.max(1, Math.abs(ticks[0] ?? 1));
  return Math.abs((ticks[1] ?? 1) - (ticks[0] ?? 0));
}

function timeTickLabel(value: number, step: number): string {
  if (!Number.isFinite(value)) return "";
  if (Math.abs(value) >= 1e6 || (value !== 0 && Math.abs(value) < 1e-4)) return shortNumber(value, 4);
  let decimals = Math.max(0, -Math.floor(Math.log10(Math.max(Number.MIN_VALUE, step))));
  while (decimals < 8 && Math.abs(step * 10 ** decimals - Math.round(step * 10 ** decimals)) > 1e-8) decimals += 1;
  return new Intl.NumberFormat(undefined, { minimumFractionDigits: 0, maximumFractionDigits: decimals }).format(value);
}

function guideFrequencyTicks(minimum: number, maximum: number, plotHeight: number): number[] {
  if (!Number.isFinite(minimum) || !Number.isFinite(maximum) || minimum <= 0 || maximum <= minimum) return [];
  const firstExponent = Math.floor(Math.log10(minimum));
  const lastExponent = Math.ceil(Math.log10(maximum));
  if (lastExponent - firstExponent > 32) return [];
  const ticks: number[] = [];
  for (let exponent = firstExponent; exponent <= lastExponent; exponent += 1) {
    const decade = 10 ** exponent;
    for (const multiplier of [1, 2, 5]) {
      const value = multiplier * decade;
      if (value >= minimum * (1 - 1e-12) && value <= maximum * (1 + 1e-12)) ticks.push(value);
    }
  }
  const maximumLabels = Math.max(2, Math.floor(plotHeight / 30));
  if (ticks.length <= maximumLabels) return ticks;
  const decadeTicks = ticks.filter((value) => Math.abs(Math.log10(value) - Math.round(Math.log10(value))) < 1e-9);
  if (decadeTicks.length <= maximumLabels) return decadeTicks;
  const stride = Math.ceil(decadeTicks.length / maximumLabels);
  return decadeTicks.filter((_value, index) => index % stride === 0);
}

function formatFrequency(value: number): string {
  return `${shortNumber(value, 6)} Hz`;
}

function configurationSummary(config: ResponseConfig): string {
  const topology = enumName(config.topology);
  const response = config.response === "chebyshev1" ? "Chebyshev I" : enumName(config.response);
  const cutoff = config.topology === "band-pass"
    ? `${formatFrequency(config.lower_cutoff_hz ?? NaN)}–${formatFrequency(config.upper_cutoff_hz ?? NaN)}`
    : formatFrequency(config.cutoff_hz ?? NaN);
  return `${topology} · ${response} · order ${config.order} · ${cutoff}`;
}

function frequencyInputStep(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 0.1;
  return 10 ** (Math.floor(Math.log10(value)) - 1);
}

function refreshFrequencyInputStep(input: HTMLInputElement): void {
  input.step = String(frequencyInputStep(Number(input.value)));
}

function timeUnit(values: number[]): { factor: number; label: string } {
  const largest = Math.max(0, ...values.filter(Number.isFinite).map((value) => Math.abs(value)));
  if (largest >= 1) return { factor: 1, label: "s" };
  if (largest >= 1e-3) return { factor: 1e3, label: "ms" };
  if (largest >= 1e-6) return { factor: 1e6, label: "µs" };
  return { factor: 1e9, label: "ns" };
}

function enumName(value: string): string {
  return value
    .split("-")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join("-");
}

function currentTopology(): Topology {
  return ui.topology.value as Topology;
}

function currentResponse(): ResponseFamily {
  return ui.response.value as ResponseFamily;
}

function besselMaxOrder(topology: Topology): number {
  return topology === "band-pass" ? 50 : 25;
}

function maxOrder(topology: Topology, response: ResponseFamily): number {
  return response === "bessel" ? besselMaxOrder(topology) : 50;
}

function isValidOrder(order: number, topology: Topology, response: ResponseFamily): boolean {
  const max = maxOrder(topology, response);
  return Number.isInteger(order) && order > 0 && order <= max && (topology !== "band-pass" || order % 2 === 0);
}

function refreshOrderConstraints(preferredOrder?: number): void {
  const topology = currentTopology();
  const response = currentResponse();
  const previous = preferredOrder ?? Number(ui.order.value);
  const limit = maxOrder(topology, response);
  const minimum = topology === "band-pass" ? 2 : 1;
  ui.order.min = String(minimum);
  ui.order.max = String(limit);
  ui.order.step = topology === "band-pass" ? "2" : "1";
  ui.order.value = String(isValidOrder(previous, topology, response) ? previous : 4);
}

function updateConditionalControls(): void {
  const bandPass = currentTopology() === "band-pass";
  const chebyshev = currentResponse() === "chebyshev1";
  ui.singleCutoffField.hidden = bandPass;
  ui.bandCutoffFields.hidden = !bandPass;
  ui.rippleField.hidden = !chebyshev;
  ui.topologyHint.textContent = bandPass
    ? "Keeps a range between two complete-filter −3 dB edges. Final order must be even."
    : currentTopology() === "high-pass"
      ? "Removes slow baseline drift; the endpoint uses the new sample."
      : "Smooths high-frequency variation. The cutoff is the whole-filter −3 dB point.";
  ui.inputModelHint.textContent = ui.inputModel.value === "previous-hold"
    ? "The preceding sample drives the interval; the new sample is the endpoint."
    : ui.inputModel.value === "current-hold"
      ? "The new sample is treated as held across the preceding interval."
      : "The signal is interpolated between its two sample values.";
  getElement<HTMLParagraphElement>("order-hint").textContent = bandPass
    ? currentResponse() === "bessel"
      ? "Band-pass final order is even; Bessel supports final orders through 50."
      : "Band-pass final order is positive and even; supported orders are 2 through 50."
    : currentResponse() === "bessel"
      ? "Bessel low-pass and high-pass orders are supported through 25."
      : "Choose a positive integer order from 1 through 50.";
}

function showError(message: string): void {
  responseRequestId += 1;
  const preservesResponse = responseResult !== undefined && appliedConfig !== undefined;
  const displayedMessage = preservesResponse
    ? `${message} The plot and loaded samples still show the last valid setup.`
    : message;
  ui.validation.textContent = displayedMessage;
  ui.validation.hidden = false;
  ui.responseSummary.textContent = displayedMessage;
  ui.responseSummary.classList.add("is-error");
  if (!preservesResponse) {
    frequencyPlot?.destroy();
    stepPlot?.destroy();
    frequencyPlot = undefined;
    stepPlot = undefined;
    ui.frequencyChartHost.replaceChildren();
    ui.stepChartHost.replaceChildren();
    ui.chartHost.setAttribute("aria-label", "Response chart unavailable.");
    ui.responseChartSummary.textContent = message;
    if (loadedCsv) showFilterPlaceholder("The filter setup needs attention", "Correct the highlighted setting above to re-filter these samples.");
    filterRequestId += 1;
    responseResult = undefined;
  }
}

function clearError(): void {
  ui.validation.textContent = "";
  ui.validation.hidden = true;
  ui.responseSummary.classList.remove("is-error");
}

function validateConfiguration(): ResponseConfig | undefined {
  const topology = currentTopology();
  const response = currentResponse();
  const order = Number(ui.order.value);
  if (!isValidOrder(order, topology, response)) {
    showError(topology === "band-pass"
      ? `Band-pass order must be an even integer from 2 through ${maxOrder(topology, response)}.`
      : `This response supports orders 1 through ${maxOrder(topology, response)}.`);
    return undefined;
  }

  const config: ResponseConfig = { topology, response, order, input_model: ui.inputModel.value as InputModel };
  if (topology === "band-pass") {
    const lower = Number(ui.lowerCutoff.value);
    const upper = Number(ui.upperCutoff.value);
    if (!Number.isFinite(lower) || !Number.isFinite(upper) || lower <= 0 || upper <= lower) {
      showError("Enter positive, finite band edges with the upper edge greater than the lower edge.");
      return undefined;
    }
    config.lower_cutoff_hz = lower;
    config.upper_cutoff_hz = upper;
  } else {
    const cutoff = Number(ui.cutoff.value);
    if (!Number.isFinite(cutoff) || cutoff <= 0) {
      showError("Cutoff frequency must be a positive, finite number.");
      return undefined;
    }
    config.cutoff_hz = cutoff;
  }

  if (response === "chebyshev1") {
    const ripple = Number(ui.ripple.value);
    if (!Number.isFinite(ripple) || ripple <= 0 || ripple >= 3.0103) {
      showError("Chebyshev I ripple must be greater than 0 and less than 3.0103 dB.");
      return undefined;
    }
    config.ripple_db = ripple;
  }

  return config;
}

function currentFilterConfig(base: ResponseConfig): FilterConfig | undefined {
  const initialMode = selectedInitialMode();
  let initialValue = Number(ui.initialValue.value);
  if (initialMode === "sample0") {
    initialValue = loadedCsv?.values[0] ?? 0;
  } else {
    const firstInterval = Number(ui.firstInterval.value);
    if (ui.initialValue.value.trim() === "" || !Number.isFinite(initialValue)) {
      showCsvError("Custom initial input must be a finite number.");
      return undefined;
    }
    if (!Number.isFinite(firstInterval) || firstInterval <= 0) {
      showCsvError("Enter a positive, finite first interval in seconds for the custom initial state.");
      return undefined;
    }
    return {
      ...base,
      initial_mode: initialMode,
      initial_value: initialValue,
      first_interval_seconds: firstInterval,
    };
  }

  return { ...base, initial_mode: initialMode, initial_value: initialValue };
}

function setStatus(message: string): void {
  ui.responseSummary.textContent = message;
  ui.responseSummary.classList.remove("is-error");
}

async function updateResponse(): Promise<void> {
  const config = validateConfiguration();
  if (!config) return;
  renderCode(config);
  clearError();
  updateUrl(config);
  updateCardHeading(config);
  const requestId = ++responseRequestId;
  setStatus("Calculating the continuous-time response…");

  try {
    adapter ??= await loadAdapter();
    if (requestId !== responseRequestId) return;
    const result = await adapter.response_data(config);
    if (requestId !== responseRequestId) return;
    assertResponseShape(result);
    appliedConfig = { ...config };
    responseResult = result;
    setStatus(configurationSummary(config));
    updateDataFilterContext(config);
    drawResponseChart();
    if (loadedCsv) await runFilter(config);
  } catch (error) {
    if (requestId !== responseRequestId) return;
    showError(adapterUnavailableMessage(error));
    if (loadedCsv) showCsvError(adapterUnavailableMessage(error));
  }
}

function scheduleResponseUpdate(delay = 140): void {
  window.clearTimeout(responseDebounceTimer);
  responseDebounceTimer = window.setTimeout(() => void updateResponse(), delay);
}

function assertResponseShape(result: ResponseResult): void {
  const series = [result.frequency_hz, result.gain_db, result.phase_degrees, result.group_delay_seconds, result.step_time_seconds, result.step_output];
  if (series.some((values) => values.length < 2)) {
    throw new Error("The Rust response adapter returned an incomplete response series.");
  }
  if (result.frequency_hz.length !== result.gain_db.length || result.frequency_hz.length !== result.phase_degrees.length || result.frequency_hz.length !== result.group_delay_seconds.length || result.step_time_seconds.length !== result.step_output.length) {
    throw new Error("The Rust response adapter returned arrays with mismatched lengths.");
  }
}

function adapterUnavailableMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  if (message.includes("Failed to fetch dynamically imported module") || message.includes("404") || message.includes("Importing a module script failed")) {
    return "The Rust WebAssembly adapter is not built yet. The playground will calculate responses after site/wasm/pkg/ssfilt_web_wasm.js is available.";
  }
  return `Could not calculate this response: ${message}`;
}

function updateCardHeading(config: ResponseConfig): void {
  const topology = enumName(config.topology);
  const response = config.response === "chebyshev1" ? "Chebyshev I" : enumName(config.response);
  ui.responseTitle.textContent = `${topology} · ${response} · order ${config.order}`;
}

function updateDataFilterContext(config: ResponseConfig): void {
  ui.dataFilterSummary.textContent = configurationSummary(config);
  ui.dataFilterNote.textContent = loadedCsv
    ? "Loaded samples are re-filtered automatically when this setup changes."
    : "Samples will use the same configuration shown above.";
}

function activeChartData(result: ResponseResult): { x: number[]; y: number[] } {
  if (activeChart === "step") return { x: numericSeries(result.step_time_seconds), y: numericSeries(result.step_output) };
  const y = activeChart === "magnitude"
    ? result.gain_db
    : activeChart === "phase"
      ? result.phase_degrees
      : result.group_delay_seconds;
  return { x: numericSeries(result.frequency_hz), y: numericSeries(y) };
}

function displayChartData(result: ResponseResult): { x: number[]; y: number[]; xLabel: string; yLabel: string } {
  const data = activeChartData(result);
  const meta = chartMeta[activeChart];
  if (activeChart === "step") {
    const unit = timeUnit(data.x);
    return { x: data.x.map((value) => value * unit.factor), y: data.y, xLabel: `Time · ${unit.label}`, yLabel: meta.y };
  }
  if (activeChart === "delay") {
    const unit = timeUnit(data.y);
    return { x: data.x, y: data.y.map((value) => value * unit.factor), xLabel: meta.x, yLabel: `Group delay · ${unit.label}` };
  }
  return { ...data, xLabel: meta.x, yLabel: meta.y };
}

function cutoffMarkerPlugin(): uPlot.Plugin {
  return {
    hooks: {
      draw: (plot) => {
        if (!appliedConfig || activeChart === "step") return;
        const cutoffs = appliedConfig.topology === "band-pass"
          ? [appliedConfig.lower_cutoff_hz, appliedConfig.upper_cutoff_hz]
          : [appliedConfig.cutoff_hz];
        const finiteCutoffs = cutoffs.filter((value): value is number => value !== undefined && Number.isFinite(value));
        const pixelRatio = window.devicePixelRatio || 1;
        const { ctx, bbox } = plot;
        ctx.save();
        finiteCutoffs.forEach((cutoff) => {
          const x = Math.round(plot.valToPos(cutoff, "x", true));
          ctx.strokeStyle = "rgba(182, 101, 37, 0.9)";
          ctx.lineWidth = Math.max(1.5, 1.5 * pixelRatio);
          ctx.setLineDash([5 * pixelRatio, 4 * pixelRatio]);
          ctx.beginPath();
          ctx.moveTo(x, bbox.top);
          ctx.lineTo(x, bbox.top + bbox.height);
          ctx.stroke();
        });
        ctx.restore();
      },
    },
  };
}

function magnitudeFloorCrossings(result: ResponseResult): number[] {
  const frequencies = numericSeries(result.frequency_hz);
  const gains = numericSeries(result.gain_db);
  const crossings: number[] = [];
  for (let index = 1; index < gains.length; index += 1) {
    const previous = gains[index - 1];
    const current = gains[index];
    if (previous === undefined || current === undefined || !Number.isFinite(previous) || !Number.isFinite(current)) continue;
    if ((previous < MAGNITUDE_DISPLAY_FLOOR_DB) === (current < MAGNITUDE_DISPLAY_FLOOR_DB)) continue;
    const previousFrequency = frequencies[index - 1];
    const currentFrequency = frequencies[index];
    if (previousFrequency === undefined || currentFrequency === undefined || previousFrequency <= 0 || currentFrequency <= 0) continue;
    const fraction = (MAGNITUDE_DISPLAY_FLOOR_DB - previous) / (current - previous);
    const logFrequency = Math.log(previousFrequency) + fraction * (Math.log(currentFrequency) - Math.log(previousFrequency));
    crossings.push(Math.exp(logFrequency));
  }
  return crossings;
}

function makeResponseSummary(result: ResponseResult, config: ResponseConfig, kind: ChartKind): string {
  const data = activeChartData(result);
  const finite = data.y.filter(Number.isFinite);
  if (kind === "magnitude") {
    const edges = config.topology === "band-pass"
      ? [config.lower_cutoff_hz, config.upper_cutoff_hz]
      : [config.cutoff_hz];
    const edgeText = edges.filter((edge): edge is number => edge !== undefined).map((edge) => {
      const index = nearestIndex(numericSeries(result.frequency_hz), edge);
      return `${formatFrequency(edge)}: ${shortNumber(numericSeries(result.gain_db)[index] ?? NaN, 3)} dB`;
    }).join("; ");
    return `Magnitude response sampled at ${data.x.length} frequencies. Approximate gain at the selected −3 dB edge${edges.length > 1 ? "s" : ""}: ${edgeText}.`;
  }
  if (kind === "phase") return `Natural phase response sampled at ${data.x.length} frequencies, ranging from ${shortNumber(Math.min(...finite), 3)} to ${shortNumber(Math.max(...finite), 3)} degrees.`;
  if (kind === "delay") return `Group delay sampled at ${data.x.length} frequencies, ranging from ${shortNumber(Math.min(...finite), 4)} to ${shortNumber(Math.max(...finite), 4)} seconds.`;
  return `Canonical current-hold step trace sampled at ${data.x.length} times. The plotted window ends at ${shortNumber(data.x.at(-1) ?? NaN, 4)} seconds with output ${shortNumber(data.y.at(-1) ?? NaN, 4)}.`;
}

function nearestIndex(values: number[], target: number): number {
  let nearest = 0;
  let smallest = Infinity;
  values.forEach((value, index) => {
    const distance = Math.abs(value - target);
    if (distance < smallest) {
      nearest = index;
      smallest = distance;
    }
  });
  return nearest;
}

function replayReveal(element: HTMLElement): void {
  element.classList.remove("is-revealing");
  // Restart the short transition for a newly selected response view.
  void element.offsetWidth;
  element.classList.add("is-revealing");
}

function responseYLimits(minimum: number, maximum: number): { min: number; max: number } {
  if (activeChart === "magnitude") return { min: MAGNITUDE_DISPLAY_FLOOR_DB, max: 5 };
  const span = maximum - minimum;
  const padding = span > 0 ? span * 0.06 : Math.max(1, Math.abs(maximum) * 0.06);
  return { min: minimum - padding, max: maximum + padding };
}

function responseYRange(_plot: uPlot, minimum: number, maximum: number): [number, number] {
  const limits = responseYLimits(minimum, maximum);
  return [limits.min, limits.max];
}

function frequencyScaleRange(
  _plot: uPlot,
  minimum: number,
  maximum: number,
): [number, number] {
  // Keep the sampled endpoints just inside the canvas while preserving the
  // selected cutoff at the geometric center of the logarithmic range.
  const expansion = 1 + 1e-6;
  return [minimum / expansion, maximum * expansion];
}

function frequencyAxisSplits(
  _plot: uPlot,
  _axisIndex: number,
  minimum: number,
  maximum: number,
): number[] {
  if (!Number.isFinite(minimum) || !Number.isFinite(maximum) || minimum <= 0 || maximum <= minimum) return [];

  const firstExponent = Math.floor(Math.log10(minimum));
  const lastExponent = Math.ceil(Math.log10(maximum));
  // Response profiles span four decades. The cap keeps malformed external
  // data from turning axis rendering into unbounded work.
  if (lastExponent - firstExponent > 32) return [minimum, maximum];

  const tolerance = 1e-12;
  const splits: number[] = [];
  for (let exponent = firstExponent; exponent <= lastExponent; exponent += 1) {
    const decade = 10 ** exponent;
    for (let multiplier = 1; multiplier < 10; multiplier += 1) {
      const value = multiplier * decade;
      if (value >= minimum * (1 - tolerance) && value <= maximum * (1 + tolerance)) splits.push(value);
    }
  }
  return splits;
}

function frequencyAxisValues(
  _plot: uPlot,
  splits: number[],
): (string | null)[] {
  return splits.map((value) => {
    const exponent = Math.log10(value);
    return Math.abs(exponent - Math.round(exponent)) < 1e-9 ? shortNumber(value, 6) : null;
  });
}

function timeAxisSplits(
  plot: uPlot,
  _axisIndex: number,
  minimum: number,
  maximum: number,
): number[] {
  const pixelRatio = window.devicePixelRatio || 1;
  return timeAxisTicks(minimum, maximum, plot.bbox.width / pixelRatio);
}

function timeAxisValues(
  _plot: uPlot,
  splits: number[],
): string[] {
  const step = tickStep(splits);
  return splits.map((value) => timeTickLabel(value, step));
}

function updateChartReferenceKey(config: ResponseConfig, result: ResponseResult): void {
  const frequencyChart = activeChart !== "step";
  ui.chartReferenceKey.hidden = !frequencyChart;
  if (!frequencyChart) return;
  ui.cutoffReferenceLabel.textContent = config.topology === "band-pass"
    ? `−3 dB edges · ${formatFrequency(config.lower_cutoff_hz ?? NaN)} / ${formatFrequency(config.upper_cutoff_hz ?? NaN)}`
    : `−3 dB cutoff · ${formatFrequency(config.cutoff_hz ?? NaN)}`;
  ui.magnitudeFloorKey.hidden = activeChart !== "magnitude" || magnitudeFloorCrossings(result).length === 0;
}

function drawResponseChart(): void {
  if (!responseResult || !appliedConfig) return;
  const meta = chartMeta[activeChart];
  const values = displayChartData(responseResult);
  if (values.x.length !== values.y.length || values.x.length === 0) return;
  ui.chartTitle.textContent = meta.title;
  ui.chartSubtitle.textContent = meta.subtitle;
  updateChartReferenceKey(appliedConfig, responseResult);
  ui.chartPanel.setAttribute("aria-labelledby", `tab-${activeChart}`);
  ui.chartHost.setAttribute("aria-label", `${meta.title}; ${makeResponseSummary(responseResult, appliedConfig, activeChart)}`);
  ui.responseChartSummary.textContent = makeResponseSummary(responseResult, appliedConfig, activeChart);

  const rect = ui.chartHost.getBoundingClientRect();
  const width = Math.max(220, Math.floor(rect.width || 660));
  const height = Math.max(230, Math.min(340, Math.round(width * 0.45)));
  const stepChart = activeChart === "step";
  const frequencyRange = {
    min: values.x[0] ?? 1,
    max: values.x[values.x.length - 1] ?? 1,
  };
  ui.frequencyChartHost.hidden = stepChart;
  ui.stepChartHost.hidden = !stepChart;
  const existingPlot = stepChart ? stepPlot : frequencyPlot;
  if (existingPlot) {
    if (existingPlot.axes[0]) existingPlot.axes[0].label = values.xLabel;
    if (existingPlot.axes[1]) existingPlot.axes[1].label = values.yLabel;
    const finiteY = values.y.filter(Number.isFinite);
    const yLimits = finiteY.length > 0
      ? responseYLimits(Math.min(...finiteY), Math.max(...finiteY))
      : { min: 0, max: 1 };
    existingPlot.batch(() => {
      if (existingPlot.width !== width || existingPlot.height !== height) existingPlot.setSize({ width, height });
      existingPlot.setData([values.x, values.y], false);
      existingPlot.setScale("x", stepChart
        ? { min: values.x[0], max: values.x[values.x.length - 1] }
        : frequencyRange);
      existingPlot.setScale("y", yLimits);
    });
    return;
  }

  const plot = new uPlot({
    width,
    height,
    class: "ssfilt-plot",
    cursor: { drag: { x: false, y: false, setScale: false } },
    legend: { show: false },
    plugins: [cutoffMarkerPlugin()],
    scales: {
      x: stepChart
        ? { time: false, distr: 1 }
        : { time: false, distr: 3, range: frequencyScaleRange },
      y: { range: responseYRange },
    },
    axes: [
      {
        stroke: "#687780",
        grid: { stroke: "rgba(21, 45, 57, 0.08)", width: 1 },
        ticks: { stroke: "rgba(21, 45, 57, 0.14)", width: 1 },
        font: "11px Inter, system-ui, sans-serif",
        size: 38,
        label: values.xLabel,
        labelSize: 15,
        labelGap: 0,
        labelFont: "9px DM Mono, ui-monospace, monospace",
        splits: stepChart ? undefined : frequencyAxisSplits,
        values: stepChart ? undefined : frequencyAxisValues,
      },
      {
        stroke: "#687780",
        grid: { stroke: "rgba(21, 45, 57, 0.08)", width: 1 },
        ticks: { stroke: "rgba(21, 45, 57, 0.14)", width: 1 },
        font: "11px Inter, system-ui, sans-serif",
        size: 48,
        side: 3,
        label: values.yLabel,
        labelSize: 15,
        labelGap: 0,
        labelFont: "9px DM Mono, ui-monospace, monospace",
      },
    ],
    series: [
      {},
      { label: meta.y, stroke: stepChart ? "#bfed4b" : "#168779", width: 2.5, points: { show: false } },
    ],
  }, [values.x, values.y], stepChart ? ui.stepChartHost : ui.frequencyChartHost);
  if (stepChart) stepPlot = plot;
  else frequencyPlot = plot;
}

function renderCode(config = validateConfiguration()): void {
  if (!config) {
    ui.code.textContent = "// Correct the filter setup to see the matching Rust example.";
    return;
  }
  const topologyType = config.topology === "low-pass" ? "LowPass" : config.topology === "high-pass" ? "HighPass" : "BandPass";
  const builderArgs = config.topology === "band-pass"
    ? `${rustNumber(config.lower_cutoff_hz ?? 4)}, ${rustNumber(config.upper_cutoff_hz ?? 16)}`
    : rustNumber(config.cutoff_hz ?? 8);
  const family = config.response === "chebyshev1"
    ? `Response::Chebyshev1 { ripple_db: ${rustNumber(config.ripple_db ?? 0.5)} }`
    : `Response::${rustVariant(config.response)}`;
  const model = `InputModel::${rustVariant(config.input_model)}`;
  const initialMode = selectedInitialMode();
  const initial = initialMode === "custom"
    ? rustNumber(Number(ui.initialValue.value))
    : rustNumber(loadedCsv?.values[0] ?? 0);
  const initialization = initialMode === "custom"
    ? `    // Advance the custom state to sample 0 over this full interval.\n    let first_sample = 0.0; // Replace with the first CSV value.\n    let first_interval_seconds = ${rustNumber(Number(ui.firstInterval.value))};\n    let output = filter.update(first_sample, first_interval_seconds)?;`
    : `    // Start in steady state at sample 0, then advance to sample 1.\n    let next_sample = 0.0; // Replace with the next CSV value.\n    let elapsed_seconds = 0.01; // Difference between adjacent timestamps.\n    let output = filter.update(next_sample, elapsed_seconds)?;`;
  const imports = `use ssfilt::{${topologyType}, InputModel, Response};`;
  const code = `${imports}\n\nfn main() -> Result<(), Box<dyn std::error::Error>> {\n    // Every public cutoff is a complete-filter −3 dB point.\n    let mut filter = ${topologyType}::<${config.order}>::builder(${builderArgs})\n        .response(${family})\n        .input_model(${model})\n        .initial_input(${initial})\n        .build()?;\n\n${initialization}\n    let _ = output;\n    Ok(())\n}`;
  ui.code.innerHTML = hljs.highlight(code, { language: "rust" }).value;
}

function rustVariant(value: ResponseFamily | InputModel): string {
  const names: Record<ResponseFamily | InputModel, string> = {
    "repeated-pole": "RepeatedPole",
    butterworth: "Butterworth",
    bessel: "Bessel",
    chebyshev1: "Chebyshev1",
    linear: "Linear",
    "previous-hold": "PreviousHold",
    "current-hold": "CurrentHold",
  };
  return names[value];
}

function rustNumber(value: number): string {
  if (!Number.isFinite(value)) return "0.0";
  return Number.isInteger(value) ? `${value}.0` : value.toString();
}

function setActiveChart(kind: ChartKind): void {
  activeChart = kind;
  chartTabs.forEach((tab) => {
    const selected = tab.dataset.chart === kind;
    tab.setAttribute("aria-selected", String(selected));
    tab.tabIndex = selected ? 0 : -1;
  });
  if (responseResult && appliedConfig) {
    setStatus(configurationSummary(appliedConfig));
    drawResponseChart();
  }
}

function updateUrl(config: ResponseConfig): void {
  const url = new URL(window.location.href);
  url.searchParams.set("topology", config.topology);
  url.searchParams.set("response", config.response);
  url.searchParams.set("order", String(config.order));
  url.searchParams.set("input_model", config.input_model);
  if (config.cutoff_hz !== undefined) {
    url.searchParams.set("cutoff_hz", String(config.cutoff_hz));
    url.searchParams.delete("lower_cutoff_hz");
    url.searchParams.delete("upper_cutoff_hz");
  } else {
    url.searchParams.set("lower_cutoff_hz", String(config.lower_cutoff_hz));
    url.searchParams.set("upper_cutoff_hz", String(config.upper_cutoff_hz));
    url.searchParams.delete("cutoff_hz");
  }
  if (config.ripple_db !== undefined) url.searchParams.set("ripple_db", String(config.ripple_db));
  else url.searchParams.delete("ripple_db");
  const initialMode = selectedInitialMode();
  url.searchParams.set("initial_mode", initialMode);
  if (initialMode === "custom") {
    url.searchParams.set("initial_value", ui.initialValue.value);
    url.searchParams.set("first_interval_seconds", ui.firstInterval.value);
  } else {
    url.searchParams.delete("initial_value");
    url.searchParams.delete("first_interval_seconds");
  }
  window.history.replaceState(null, "", url);
}

function persistFilterConfiguration(): void {
  const config = validateConfiguration();
  if (config) updateUrl(config);
}

function restoreUrl(): void {
  const params = new URLSearchParams(window.location.search);
  const topology = params.get("topology");
  const response = params.get("response");
  const inputModel = params.get("input_model");
  if (["low-pass", "high-pass", "band-pass"].includes(topology ?? "")) ui.topology.value = topology ?? "low-pass";
  if (["repeated-pole", "butterworth", "bessel", "chebyshev1"].includes(response ?? "")) ui.response.value = response ?? "butterworth";
  if (["linear", "previous-hold", "current-hold"].includes(inputModel ?? "")) ui.inputModel.value = inputModel ?? "linear";
  refreshOrderConstraints(Number(params.get("order")) || 4);
  const cutoff = Number(params.get("cutoff_hz"));
  const lower = Number(params.get("lower_cutoff_hz"));
  const upper = Number(params.get("upper_cutoff_hz"));
  const ripple = Number(params.get("ripple_db"));
  if (Number.isFinite(cutoff) && cutoff > 0) ui.cutoff.value = String(cutoff);
  if (Number.isFinite(lower) && lower > 0) ui.lowerCutoff.value = String(lower);
  if (Number.isFinite(upper) && upper > 0) ui.upperCutoff.value = String(upper);
  if (Number.isFinite(ripple) && ripple > 0 && ripple < 3.0103) ui.ripple.value = String(ripple);
  const initialMode = params.get("initial_mode");
  if (initialMode === "sample0" || initialMode === "custom") selectInitialMode(initialMode);
  const initialValue = Number(params.get("initial_value"));
  const firstInterval = Number(params.get("first_interval_seconds"));
  if (Number.isFinite(initialValue)) ui.initialValue.value = String(initialValue);
  if (Number.isFinite(firstInterval) && firstInterval > 0) ui.firstInterval.value = String(firstInterval);
}

function showToast(message: string): void {
  window.clearTimeout(toastTimer);
  ui.toast.textContent = message;
  ui.toast.classList.add("is-visible");
  toastTimer = window.setTimeout(() => ui.toast.classList.remove("is-visible"), 2400);
}

async function copyText(value: string, label: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(value);
    showToast(`${label} copied to clipboard`);
  } catch {
    showToast(`Copy was unavailable. Select and copy the ${label.toLowerCase()} manually.`);
  }
}

function updateInitialMode(): void {
  const custom = selectedInitialMode() === "custom";
  ui.customInitialFields.hidden = !custom;
  ui.initialValue.required = custom;
  ui.firstInterval.required = custom;
  ui.initialModeDescription.textContent = custom
    ? "Set a finite steady input before sample 0, then provide the elapsed time to the first sample."
    : "Row 1 establishes the steady-state output; integration begins with row 2.";
  renderCode();
}

function toggleCodeView(): void {
  const expanded = ui.codeToggle.getAttribute("aria-expanded") !== "true";
  ui.codeToggle.setAttribute("aria-expanded", String(expanded));
  ui.codeToggle.textContent = expanded ? "Collapse code" : "Show full code";
  ui.codeCard.classList.toggle("is-code-expanded", expanded);
}

function updateCsvLineNumbers(): void {
  const lineCount = (ui.csvText.value.match(/\n/g)?.length ?? 0) + 1;
  ui.csvLineNumbers.textContent = Array.from({ length: lineCount }, (_, index) => String(index + 1)).join("\n");
  ui.csvLineNumbers.scrollTop = ui.csvText.scrollTop;
}

function syncCsvLineNumberScroll(): void {
  ui.csvLineNumbers.scrollTop = ui.csvText.scrollTop;
}

function showFilterPlaceholder(title: string, copy: string): void {
  ui.filteredPlaceholderTitle.textContent = title;
  ui.filteredPlaceholderCopy.textContent = copy;
  ui.filteredPlaceholder.hidden = false;
  ui.filteredContent.hidden = true;
  ui.downloadCsv.hidden = true;
}

function showFilterResult(): void {
  const firstReveal = ui.filteredContent.hidden;
  ui.filteredPlaceholder.hidden = true;
  ui.filteredContent.hidden = false;
  ui.downloadCsv.hidden = false;
  if (firstReveal) replayReveal(ui.filteredContent);
}

function showCsvError(message: string): void {
  ui.csvError.textContent = message;
  ui.csvError.hidden = false;
  ui.csvStatus.textContent = "CSV was not applied.";
  ui.csvStatus.classList.add("is-error");
}

function clearCsvError(): void {
  ui.csvError.textContent = "";
  ui.csvError.hidden = true;
  ui.csvStatus.classList.remove("is-error");
}

function discardLoadedCsv(): void {
  loadedCsv = undefined;
  filterResult = undefined;
  filterRequestId += 1;
  frequencyGuideRequestId += 1;
  signalRevision += 1;
  analyzedSignalRevision = -1;
  analyzedInputModel = undefined;
  frequencyGuideResult = undefined;
  filterPlot?.destroy();
  filterPlot = undefined;
  ui.filterChart.replaceChildren();
  clearFrequencyGuideCanvas();
  ui.frequencyGuideStatus.textContent = "Load and filter a signal to build the frequency guide.";
  showFilterPlaceholder("Your filtered trace will appear here", "Paste or generate a signal, then filter it with the active setup above.");
  if (appliedConfig) updateDataFilterContext(appliedConfig);
}

function loadCsvText(text: string): void {
  clearCsvError();
  updateCsvLineNumbers();
  try {
    const parsed = parseCsv(text);
    const preservesDisplayedResult = filterResult !== undefined
      && filterPlot !== undefined
      && !ui.filteredContent.hidden;
    loadedCsv = parsed;
    signalRevision += 1;
    frequencyGuideRequestId += 1;
    frequencyGuideResult = undefined;
    analyzedSignalRevision = -1;
    if (frequencyGuideOpen) {
      clearFrequencyGuideCanvas();
      ui.frequencyGuideStatus.textContent = "Waiting for the new samples to finish filtering…";
    }
    ui.csvStatus.textContent = `${parsed.rowCount.toLocaleString()} samples ready · ${shortNumber(parsed.times[0] ?? 0, 5)} to ${shortNumber(parsed.times.at(-1) ?? 0, 5)} seconds`;
    if (!preservesDisplayedResult) {
      filterResult = undefined;
      filterPlot?.destroy();
      filterPlot = undefined;
      ui.filterChart.replaceChildren();
      showFilterPlaceholder("Filtering your signal…", "Every irregular interval is being integrated with the active setup.");
    }
    if (appliedConfig) updateDataFilterContext(appliedConfig);
    renderCode();
    if (appliedConfig) void runFilter(appliedConfig);
    else scheduleResponseUpdate(0);
  } catch (error) {
    discardLoadedCsv();
    showCsvError(error instanceof Error ? error.message : String(error));
  }
}

function exampleCsv(kind: ExampleSignal): string {
  let state = 0x5f37_59df;
  const noise = (): number => {
    state = (Math.imul(state, 1_664_525) + 1_013_904_223) >>> 0;
    return (state / 0xffff_ffff) * 2 - 1;
  };
  const lines = ["time,value"];
  let time = 0;

  for (let index = 0; index < 420; index += 1) {
    let value: number;
    if (kind === "step-drift") {
      value = (time >= 1.35 ? 1 : 0) + 0.18 * Math.sin(Math.PI * 0.38 * time) + 0.055 * noise();
    } else if (kind === "motion-pulses") {
      const pulse = (center: number, width: number, height: number): number =>
        height * Math.exp(-(((time - center) / width) ** 2));
      value = 0.12 * Math.sin(Math.PI * 0.7 * time)
        + pulse(0.75, 0.09, 0.9)
        - pulse(1.55, 0.14, 0.65)
        + pulse(2.5, 0.08, 1.15)
        + 0.08 * noise();
    } else {
      value = Math.sin(Math.PI * 2.2 * time)
        + 0.3 * Math.sin(Math.PI * 36 * time)
        + 0.12 * noise();
    }
    lines.push(`${time.toFixed(6)},${value.toFixed(6)}`);
    time += 0.007 + 0.009 * ((noise() + 1) / 2);
  }

  return lines.join("\n");
}

function loadExampleSignal(): void {
  const text = exampleCsv(ui.exampleSignal.value as ExampleSignal);
  ui.csvText.value = text;
  loadCsvText(text);
}

async function handleCsvFile(file: File): Promise<void> {
  if (file.size > MAX_CSV_BYTES) {
    discardLoadedCsv();
    showCsvError("This CSV is larger than 2 MB. Reduce the file size and try again.");
    return;
  }
  try {
    const text = await file.text();
    ui.csvText.value = text;
    loadCsvText(text);
  } catch (error) {
    discardLoadedCsv();
    showCsvError(`Could not read this file: ${error instanceof Error ? error.message : String(error)}`);
  }
}

async function runFilter(baseConfig?: ResponseConfig): Promise<void> {
  if (!loadedCsv) return;
  const requestId = ++filterRequestId;
  const config = baseConfig ?? validateConfiguration();
  if (!config) {
    showFilterPlaceholder("The filter setup needs attention", "Correct the highlighted setting above to filter these samples.");
    return;
  }
  const filterConfig = currentFilterConfig(config);
  if (!filterConfig) {
    showFilterPlaceholder("The startup state needs attention", "Correct the initial condition to filter these samples.");
    return;
  }
  const preservesDisplayedResult = filterResult !== undefined
    && filterPlot !== undefined
    && !ui.filteredContent.hidden;
  if (!preservesDisplayedResult) {
    showFilterPlaceholder("Filtering your signal…", "Every irregular interval is being integrated with the active setup.");
  }
  ui.csvStatus.textContent = "Filtering every elapsed interval…";

  try {
    adapter ??= await loadAdapter();
    if (requestId !== filterRequestId || !loadedCsv) return;
    const result = await adapter.filter_samples(filterConfig, loadedCsv.times, loadedCsv.values);
    if (requestId !== filterRequestId) return;
    assertFilterShape(result);
    filterResult = result;
    ui.csvStatus.textContent = `${loadedCsv.rowCount.toLocaleString()} samples filtered successfully.`;
    showFilterResult();
    showDiagnostics(result);
    drawFilterChart(result);
    renderCode(config);
    if (frequencyGuideOpen) await updateFrequencyGuide(config);
  } catch (error) {
    if (requestId !== filterRequestId) return;
    filterResult = undefined;
    showFilterPlaceholder("This signal could not be filtered", "Review the CSV and filter settings, then try again.");
    showCsvError(`Could not filter this signal: ${error instanceof Error ? error.message : String(error)}`);
  }
}

function assertFilterShape(result: FilterResult): void {
  if (result.time.length < 2 || result.time.length !== result.input.length || result.time.length !== result.output.length) {
    throw new Error("The Rust adapter returned mismatched or incomplete filtered sample arrays.");
  }
}

function showDiagnostics(result: FilterResult): void {
  getElement<HTMLElement>("diag-accepted").textContent = result.accepted_steps.toLocaleString();
  getElement<HTMLElement>("diag-rejected").textContent = result.rejected_steps.toLocaleString();
  getElement<HTMLElement>("diag-derivatives").textContent = result.derivative_evaluations.toLocaleString();
  getElement<HTMLElement>("diag-equilibrium").textContent = result.equilibrium_shortcuts.toLocaleString();
}

function clearFrequencyGuideCanvas(): void {
  const context = ui.frequencyGuideCanvas.getContext("2d");
  context?.clearRect(0, 0, ui.frequencyGuideCanvas.width, ui.frequencyGuideCanvas.height);
}

function assertFrequencyGuideShape(result: FrequencyGuideResult): void {
  const times = result.time_seconds.length;
  const frequencies = result.frequency_hz.length;
  if (times < 2 || frequencies < 2 || result.magnitude_db.length !== times * frequencies) {
    throw new Error("The Rust adapter returned an incomplete frequency guide.");
  }
  if (result.gap_start_seconds.length !== result.gap_end_seconds.length) {
    throw new Error("The Rust adapter returned mismatched sparse-interval markers.");
  }
}

function inputModelLabel(model: InputModel): string {
  if (model === "previous-hold") return "previous-value hold";
  if (model === "current-hold") return "current-value hold";
  return "linear interpolation";
}

function guideColor(db: number, floor: number, alpha: number): string {
  const normalized = Math.max(0, Math.min(1, (db - floor) / -floor));
  const shaped = normalized ** 1.25;
  const hue = 164 - 129 * shaped;
  const saturation = 18 + 67 * shaped;
  const lightness = 96 - 43 * shaped;
  return `hsla(${hue}, ${saturation}%, ${lightness}%, ${alpha})`;
}

function cutoffFrequencies(config: ResponseConfig): number[] {
  return config.topology === "band-pass"
    ? [config.lower_cutoff_hz, config.upper_cutoff_hz].filter((value): value is number => value !== undefined)
    : [config.cutoff_hz].filter((value): value is number => value !== undefined);
}

function filterPlotHorizontalMargins(): { left: number; right: number } {
  if (!filterPlot) return { left: 62, right: 0 };
  const pixelRatio = window.devicePixelRatio || 1;
  const left = filterPlot.bbox.left / pixelRatio;
  const right = filterPlot.width - (filterPlot.bbox.left + filterPlot.bbox.width) / pixelRatio;
  return { left: Math.max(0, left), right: Math.max(0, right) };
}

function drawFrequencyGuide(config = appliedConfig): void {
  if (!frequencyGuideResult || !config || !frequencyGuideOpen) return;
  const result = frequencyGuideResult;
  const times = numericSeries(result.time_seconds);
  const frequencies = numericSeries(result.frequency_hz);
  const magnitude = numericSeries(result.magnitude_db);
  const canvas = ui.frequencyGuideCanvas;
  const context = canvas.getContext("2d");
  if (!context) return;

  const cssWidth = Math.max(280, Math.floor(canvas.getBoundingClientRect().width || 760));
  const cssHeight = Math.max(230, Math.floor(canvas.getBoundingClientRect().height || 280));
  const pixelRatio = Math.max(1, window.devicePixelRatio || 1);
  canvas.width = Math.round(cssWidth * pixelRatio);
  canvas.height = Math.round(cssHeight * pixelRatio);
  context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
  context.clearRect(0, 0, cssWidth, cssHeight);

  const horizontalMargins = filterPlotHorizontalMargins();
  const margin = { left: horizontalMargins.left, right: horizontalMargins.right, top: 18, bottom: 42 };
  const plotWidth = cssWidth - margin.left - margin.right;
  const plotHeight = cssHeight - margin.top - margin.bottom;
  const cellWidth = plotWidth / times.length;
  const cellHeight = plotHeight / frequencies.length;
  const firstTime = times[0] ?? 0;
  const lastTime = times.at(-1) ?? firstTime;
  const duration = Math.max(Number.EPSILON, lastTime - firstTime);
  const minimumFrequency = frequencies[0] ?? 1;
  const maximumFrequency = frequencies.at(-1) ?? minimumFrequency;
  const logMinimum = Math.log(minimumFrequency);
  const logSpan = Math.max(Number.EPSILON, Math.log(maximumFrequency) - logMinimum);

  context.fillStyle = "#f6f8f5";
  context.fillRect(margin.left, margin.top, plotWidth, plotHeight);
  frequencies.forEach((frequency, frequencyIndex) => {
    const scale = 6 / (Math.PI * 2 * frequency);
    times.forEach((time, timeIndex) => {
      const edgeDistance = Math.min(time - firstTime, lastTime - time);
      const support = Math.max(Number.EPSILON, 2 * scale);
      const reliability = Math.max(0, Math.min(1, edgeDistance / support));
      const alpha = 0.22 + 0.78 * Math.sqrt(reliability);
      const db = magnitude[frequencyIndex * times.length + timeIndex] ?? result.display_floor_db;
      context.fillStyle = guideColor(db, result.display_floor_db, alpha);
      const x = margin.left + timeIndex * cellWidth;
      const y = margin.top + plotHeight - (frequencyIndex + 1) * cellHeight;
      context.fillRect(x, y, Math.ceil(cellWidth + 0.5), Math.ceil(cellHeight + 0.5));
    });
  });

  const gapStarts = numericSeries(result.gap_start_seconds);
  const gapEnds = numericSeries(result.gap_end_seconds);
  gapStarts.forEach((start, index) => {
    const end = gapEnds[index] ?? start;
    const x = margin.left + ((start - firstTime) / duration) * plotWidth;
    const width = Math.max(1, ((end - start) / duration) * plotWidth);
    context.fillStyle = "rgba(255, 255, 255, 0.58)";
    context.fillRect(x, margin.top, width, plotHeight);
    context.strokeStyle = "rgba(78, 105, 106, 0.38)";
    context.lineWidth = 1;
    context.setLineDash([3, 3]);
    context.strokeRect(x + 0.5, margin.top + 0.5, Math.max(0, width - 1), plotHeight - 1);
  });

  const frequencyTicks = guideFrequencyTicks(minimumFrequency, maximumFrequency, plotHeight);
  const timeTicks = timeAxisTicks(firstTime, lastTime, plotWidth);
  context.setLineDash([]);
  context.strokeStyle = "rgba(21, 45, 57, 0.1)";
  context.lineWidth = 1;
  frequencyTicks.forEach((frequency) => {
    const fraction = (Math.log(frequency) - logMinimum) / logSpan;
    const y = margin.top + (1 - fraction) * plotHeight;
    context.beginPath();
    context.moveTo(margin.left, y);
    context.lineTo(margin.left + plotWidth, y);
    context.stroke();
  });
  timeTicks.forEach((time) => {
    const x = margin.left + ((time - firstTime) / duration) * plotWidth;
    context.beginPath();
    context.moveTo(x, margin.top);
    context.lineTo(x, margin.top + plotHeight);
    context.stroke();
  });

  context.font = "9px DM Mono, ui-monospace, monospace";
  context.textBaseline = "middle";
  context.setLineDash([5, 4]);
  cutoffFrequencies(config).forEach((cutoff) => {
    if (cutoff < minimumFrequency || cutoff > maximumFrequency) return;
    const fraction = (Math.log(cutoff) - logMinimum) / logSpan;
    const y = margin.top + (1 - fraction) * plotHeight;
    context.strokeStyle = "rgba(182, 101, 37, 0.95)";
    context.lineWidth = 1.5;
    context.beginPath();
    context.moveTo(margin.left, y);
    context.lineTo(margin.left + plotWidth, y);
    context.stroke();
    const label = `−3 dB · ${shortNumber(cutoff, 4)} Hz`;
    const labelWidth = context.measureText(label).width + 8;
    const labelY = y < margin.top + 14 ? y + 9 : y - 8;
    context.fillStyle = "rgba(255, 252, 247, 0.9)";
    context.fillRect(margin.left + plotWidth - labelWidth, labelY - 7, labelWidth, 14);
    context.fillStyle = "#8d5428";
    context.textAlign = "right";
    context.fillText(label, margin.left + plotWidth - 4, labelY);
  });
  context.setLineDash([]);

  context.strokeStyle = "rgba(32, 60, 68, 0.22)";
  context.lineWidth = 1;
  context.strokeRect(margin.left + 0.5, margin.top + 0.5, plotWidth - 1, plotHeight - 1);
  context.fillStyle = "#718086";
  context.font = "11px Inter, system-ui, sans-serif";
  context.textAlign = "right";
  context.textBaseline = "middle";
  frequencyTicks.forEach((frequency) => {
    const fraction = (Math.log(frequency) - logMinimum) / logSpan;
    const y = margin.top + (1 - fraction) * plotHeight;
    context.fillText(shortNumber(frequency, 4), margin.left - 7, y);
  });
  context.textAlign = "center";
  context.textBaseline = "top";
  const timeStep = tickStep(timeTicks);
  timeTicks.forEach((time) => {
    const x = margin.left + ((time - firstTime) / duration) * plotWidth;
    context.fillText(timeTickLabel(time, timeStep), x, margin.top + plotHeight + 8);
  });
  context.fillStyle = "#859197";
  context.font = "9px DM Mono, ui-monospace, monospace";
  context.save();
  context.translate(9, margin.top + plotHeight / 2);
  context.rotate(-Math.PI / 2);
  context.textAlign = "center";
  context.textBaseline = "top";
  context.fillText("Frequency · Hz", 0, 0);
  context.restore();
  context.textAlign = "center";
  context.textBaseline = "bottom";
  context.fillText("Time · s", margin.left + plotWidth / 2, cssHeight - 1);

  const shownCutoffs = cutoffFrequencies(config).filter(
    (cutoff) => cutoff >= minimumFrequency && cutoff <= maximumFrequency,
  );
  const cutoffNote = shownCutoffs.length === cutoffFrequencies(config).length
    ? "The active cutoff reference is inside the analyzed frequency range."
    : "At least one active cutoff is outside the frequency range supported by this capture.";
  const gapNote = gapStarts.length === 0
    ? "No interval exceeded four times the median sample spacing."
    : `${gapStarts.length} unusually sparse interval${gapStarts.length === 1 ? " is" : "s are"} shaded.`;
  ui.frequencyGuideStatus.textContent = `${result.summary} Reconstructed with ${inputModelLabel(config.input_model)}. ${gapNote} ${cutoffNote}`;
  canvas.setAttribute(
    "aria-label",
    `Relative input frequency guide from ${shortNumber(minimumFrequency, 3)} to ${shortNumber(maximumFrequency, 3)} hertz across ${shortNumber(firstTime, 4)} to ${shortNumber(lastTime, 4)} seconds. ${gapNote}`,
  );
}

async function updateFrequencyGuide(config: ResponseConfig): Promise<void> {
  if (!frequencyGuideOpen || !loadedCsv) return;
  if (
    frequencyGuideResult
    && analyzedSignalRevision === signalRevision
    && analyzedInputModel === config.input_model
  ) {
    drawFrequencyGuide(config);
    return;
  }

  const requestId = ++frequencyGuideRequestId;
  ui.frequencyGuideStatus.textContent = "Reconstructing the input and calculating a bounded wavelet guide…";
  await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
  try {
    adapter ??= await loadAdapter();
    if (requestId !== frequencyGuideRequestId || !loadedCsv) return;
    const result = await adapter.frequency_guide(config, loadedCsv.times, loadedCsv.values);
    if (requestId !== frequencyGuideRequestId) return;
    assertFrequencyGuideShape(result);
    frequencyGuideResult = result;
    analyzedSignalRevision = signalRevision;
    analyzedInputModel = config.input_model;
    drawFrequencyGuide(config);
  } catch (error) {
    if (requestId !== frequencyGuideRequestId) return;
    frequencyGuideResult = undefined;
    clearFrequencyGuideCanvas();
    ui.frequencyGuideStatus.textContent = `Frequency guide unavailable: ${error instanceof Error ? error.message : String(error)}`;
  }
}

function toggleFrequencyGuide(): void {
  frequencyGuideOpen = !frequencyGuideOpen;
  ui.frequencyGuideToggle.setAttribute("aria-expanded", String(frequencyGuideOpen));
  ui.frequencyGuideToggle.textContent = frequencyGuideOpen ? "Hide guide" : "Show guide";
  ui.frequencyGuideContent.hidden = !frequencyGuideOpen;
  if (!frequencyGuideOpen) {
    frequencyGuideRequestId += 1;
    return;
  }
  if (!loadedCsv || !appliedConfig) {
    ui.frequencyGuideStatus.textContent = "Load and filter a signal to build the frequency guide.";
    return;
  }
  void updateFrequencyGuide(appliedConfig);
}

function drawFilterChart(result: FilterResult): void {
  const time = numericSeries(result.time);
  const input = numericSeries(result.input);
  const output = numericSeries(result.output);
  const width = Math.max(220, Math.floor(ui.filterChart.getBoundingClientRect().width || 700));
  const height = Math.max(230, Math.min(380, Math.round(width * 0.4)));
  const existingPlot = filterPlot;
  const first = time[0] ?? 0;
  const last = time.at(-1) ?? first;
  if (existingPlot) {
    existingPlot.batch(() => {
      if (existingPlot.width !== width || existingPlot.height !== height) existingPlot.setSize({ width, height });
      existingPlot.setData([time, input, output], false);
      existingPlot.setScale("x", { min: first, max: last });
    });
  } else {
    filterPlot = new uPlot({
      width,
      height,
      class: "ssfilt-plot",
      cursor: { drag: { x: false, y: false, setScale: false } },
      legend: { show: false },
      scales: { x: { time: false, range: (_plot, minimum, maximum) => [minimum, maximum] } },
      axes: [
        {
          stroke: "#687780",
          grid: { stroke: "rgba(21, 45, 57, 0.08)", width: 1 },
          ticks: { stroke: "rgba(21, 45, 57, 0.14)", width: 1 },
          font: "11px Inter, system-ui, sans-serif",
          size: 38,
          label: "Time · s",
          labelSize: 15,
          labelGap: 0,
          labelFont: "9px DM Mono, ui-monospace, monospace",
          splits: timeAxisSplits,
          values: timeAxisValues,
        },
        {
          stroke: "#687780",
          grid: { stroke: "rgba(21, 45, 57, 0.08)", width: 1 },
          ticks: { stroke: "rgba(21, 45, 57, 0.14)", width: 1 },
          font: "11px Inter, system-ui, sans-serif",
          size: 48,
          side: 3,
          label: "Value",
          labelSize: 15,
          labelGap: 0,
          labelFont: "9px DM Mono, ui-monospace, monospace",
        },
      ],
      series: [
        {},
        { label: "Input", stroke: "#89979d", width: 1.5, points: { show: true, size: 3, fill: "#f8faf7", stroke: "#89979d" } },
        { label: "Output", stroke: "#168779", width: 2.5, points: { show: false } },
      ],
    }, [time, input, output], ui.filterChart);
  }
  ui.filterChart.setAttribute("aria-label", `Input and filter output across ${result.time.length} irregularly timed samples from ${shortNumber(first, 5)} to ${shortNumber(last, 5)} seconds.`);
  ui.filterChartSummary.textContent = `Input and filtered output for ${result.time.length} samples, from ${shortNumber(first, 5)} to ${shortNumber(last, 5)} seconds. Use the downloaded CSV for the exact values.`;
}

function downloadFilteredCsv(): void {
  if (!filterResult) return;
  const time = numericSeries(filterResult.time);
  const input = numericSeries(filterResult.input);
  const output = numericSeries(filterResult.output);
  const lines = ["time,value,output"];
  for (let index = 0; index < time.length; index += 1) {
    lines.push(`${time[index]},${input[index]},${output[index]}`);
  }
  const blob = new Blob([lines.join("\n")], { type: "text/csv;charset=utf-8" });
  const href = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = href;
  link.download = "ssfilt-filtered.csv";
  document.body.append(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(href), 1_000);
}

function setUpTabs(): void {
  chartTabs.forEach((tab, index) => {
    tab.addEventListener("click", () => setActiveChart(tab.dataset.chart as ChartKind));
    tab.addEventListener("keydown", (event) => {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight" && event.key !== "Home" && event.key !== "End") return;
      event.preventDefault();
      const targetIndex = event.key === "Home"
        ? 0
        : event.key === "End"
          ? chartTabs.length - 1
          : (index + (event.key === "ArrowRight" ? 1 : -1) + chartTabs.length) % chartTabs.length;
      const target = chartTabs[targetIndex];
      const kind = target?.dataset.chart as ChartKind | undefined;
      if (target && kind) {
        target.focus();
        setActiveChart(kind);
      }
    });
  });
}

function redrawChartsAfterResize(): void {
  if (resizeFrame !== 0) return;
  resizeFrame = window.requestAnimationFrame(() => {
    resizeFrame = 0;
    const nextResponseWidth = Math.floor(ui.responseCard.getBoundingClientRect().width);
    const nextFilterWidth = Math.floor(ui.filteredResult.getBoundingClientRect().width);
    if (nextResponseWidth !== responseContainerWidth) {
      responseContainerWidth = nextResponseWidth;
      if (responseResult) drawResponseChart();
    }
    if (nextFilterWidth !== filterContainerWidth) {
      filterContainerWidth = nextFilterWidth;
      if (filterResult && !ui.filteredContent.hidden) {
        drawFilterChart(filterResult);
        drawFrequencyGuide();
      }
    }
  });
}

function watchResize(): void {
  responseContainerWidth = Math.floor(ui.responseCard.getBoundingClientRect().width);
  filterContainerWidth = Math.floor(ui.filteredResult.getBoundingClientRect().width);
  window.addEventListener("resize", redrawChartsAfterResize, { passive: true });
}

ui.topology.addEventListener("change", () => {
  refreshOrderConstraints();
  updateConditionalControls();
  scheduleResponseUpdate(0);
});
ui.response.addEventListener("change", () => {
  refreshOrderConstraints();
  updateConditionalControls();
  scheduleResponseUpdate(0);
});
ui.order.addEventListener("input", () => scheduleResponseUpdate());
ui.inputModel.addEventListener("change", () => {
  updateConditionalControls();
  scheduleResponseUpdate(0);
});
[ui.cutoff, ui.lowerCutoff, ui.upperCutoff].forEach((input) => input.addEventListener("input", () => {
  refreshFrequencyInputStep(input);
  scheduleResponseUpdate();
}));
ui.ripple.addEventListener("input", () => scheduleResponseUpdate());
getElement<HTMLButtonElement>("copy-link").addEventListener("click", () => void copyText(window.location.href, "Configuration link"));
getElement<HTMLButtonElement>("copy-code").addEventListener("click", () => void copyText(ui.code.textContent ?? "", "Rust code"));
ui.codeToggle.addEventListener("click", toggleCodeView);
ui.initialModes.forEach((input) => input.addEventListener("change", () => {
  if (!input.checked) return;
  updateInitialMode();
  persistFilterConfiguration();
  if (loadedCsv) void runFilter();
}));
[ui.initialValue, ui.firstInterval].forEach((input) => input.addEventListener("change", () => {
  renderCode();
  persistFilterConfiguration();
  if (loadedCsv) void runFilter();
}));
getElement<HTMLButtonElement>("load-csv").addEventListener("click", () => loadCsvText(ui.csvText.value));
getElement<HTMLButtonElement>("load-example").addEventListener("click", loadExampleSignal);
ui.csvFile.addEventListener("change", () => {
  const file = ui.csvFile.files?.[0];
  if (file) void handleCsvFile(file);
});
ui.downloadCsv.addEventListener("click", downloadFilteredCsv);
ui.frequencyGuideToggle.addEventListener("click", toggleFrequencyGuide);
ui.csvText.addEventListener("input", updateCsvLineNumbers);
ui.csvText.addEventListener("scroll", syncCsvLineNumberScroll, { passive: true });

restoreUrl();
refreshOrderConstraints(Number(new URLSearchParams(window.location.search).get("order")) || 4);
updateConditionalControls();
updateInitialMode();
[ui.cutoff, ui.lowerCutoff, ui.upperCutoff].forEach(refreshFrequencyInputStep);
updateCsvLineNumbers();
setUpTabs();
watchResize();
renderCode();
void updateResponse();

window.addEventListener("beforeunload", () => {
  frequencyPlot?.destroy();
  stepPlot?.destroy();
  filterPlot?.destroy();
  window.removeEventListener("resize", redrawChartsAfterResize);
  if (resizeFrame !== 0) window.cancelAnimationFrame(resizeFrame);
});
