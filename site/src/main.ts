import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import hljs from "highlight.js/lib/core";
import rust from "highlight.js/lib/languages/rust";
import "./style.css";
import {
  loadAdapter,
  type FilterConfig,
  type FilterResult,
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
  chartHost: getElement<HTMLDivElement>("response-chart"),
  chartPanel: getElement<HTMLDivElement>("response-chart-panel"),
  chartTitle: getElement<HTMLHeadingElement>("chart-title"),
  chartSubtitle: getElement<HTMLParagraphElement>("chart-subtitle"),
  chartUnit: getElement<HTMLSpanElement>("chart-unit"),
  xAxisLabel: getElement<HTMLSpanElement>("x-axis-label"),
  yAxisLabel: getElement<HTMLSpanElement>("y-axis-label"),
  responseChartSummary: getElement<HTMLElement>("response-chart-summary"),
  code: getElement<HTMLElement>("rust-code"),
  codeCard: document.querySelector<HTMLElement>(".code-card")!,
  codeToggle: getElement<HTMLButtonElement>("toggle-code"),
  dataFilterSummary: getElement<HTMLElement>("data-filter-summary"),
  dataFilterNote: getElement<HTMLElement>("data-filter-note"),
  initialMode: getElement<HTMLSelectElement>("initial-mode"),
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
  filteredContent: getElement<HTMLDivElement>("filtered-content"),
  downloadCsv: getElement<HTMLButtonElement>("download-csv"),
  filterChart: getElement<HTMLDivElement>("filter-chart"),
  filterChartSummary: getElement<HTMLParagraphElement>("filter-chart-summary"),
  toast: getElement<HTMLDivElement>("toast"),
};

const chartTabs = [...document.querySelectorAll<HTMLButtonElement>("[role=tab][data-chart]")];
const chartMeta: Record<ChartKind, { title: string; subtitle: string; unit: string; x: string; y: string }> = {
  magnitude: { title: "Magnitude response", subtitle: "Unclamped gain · visible range ends at −140 dB", unit: "dB", x: "Frequency · Hz", y: "Gain · dB" },
  phase: { title: "Phase response", subtitle: "Natural phase shift across frequency", unit: "degrees", x: "Frequency · Hz", y: "Phase · degrees" },
  delay: { title: "Group delay", subtitle: "Delay across frequency", unit: "seconds", x: "Frequency · Hz", y: "Group delay · s" },
  step: { title: "Streamed step trace", subtitle: "Canonical current-hold step · bounded time window", unit: "output", x: "Time · s", y: "Output" },
};

let activeChart: ChartKind = "magnitude";
let adapter: SsfiltWasmModule | undefined;
let responseResult: ResponseResult | undefined;
let loadedCsv: ParsedCsv | undefined;
let filterResult: FilterResult | undefined;
let responsePlot: uPlot | undefined;
let filterPlot: uPlot | undefined;
let appliedConfig: ResponseConfig | undefined;
let toastTimer = 0;
let responseDebounceTimer = 0;
let responseRequestId = 0;
let filterRequestId = 0;
let resizeObserver: ResizeObserver | undefined;

function getElement<T extends Element>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`Required page element #${id} is missing.`);
  return element as unknown as T;
}

function numericSeries(values: NumericSeries): number[] {
  return Array.from(values, (value) => Number(value));
}

function shortNumber(value: number, digits = 3): string {
  if (!Number.isFinite(value)) return "not available";
  return new Intl.NumberFormat(undefined, { maximumSignificantDigits: digits }).format(value);
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
    ui.chartHost.replaceChildren();
    ui.chartHost.setAttribute("aria-label", "Response chart unavailable.");
    ui.responseChartSummary.textContent = message;
    if (loadedCsv) showFilterPlaceholder("The filter setup needs attention", "Correct the highlighted setting above to re-filter these samples.");
    filterRequestId += 1;
    responsePlot?.destroy();
    responsePlot = undefined;
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
  const initialMode = ui.initialMode.value as InitialMode;
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

function displayChartData(result: ResponseResult): { x: number[]; y: number[]; xLabel: string; yLabel: string; unit: string } {
  const data = activeChartData(result);
  const meta = chartMeta[activeChart];
  if (activeChart === "step") {
    const unit = timeUnit(data.x);
    return { x: data.x.map((value) => value * unit.factor), y: data.y, xLabel: `Time · ${unit.label}`, yLabel: meta.y, unit: meta.unit };
  }
  if (activeChart === "delay") {
    const unit = timeUnit(data.y);
    return { x: data.x, y: data.y.map((value) => value * unit.factor), xLabel: meta.x, yLabel: `Group delay · ${unit.label}`, unit: unit.label };
  }
  return { ...data, xLabel: meta.x, yLabel: meta.y, unit: meta.unit };
}

function drawCanvasLabel(
  ctx: CanvasRenderingContext2D,
  bbox: uPlot.BBox,
  text: string,
  anchorX: number,
  centerY: number,
  pixelRatio: number,
  colors: { background: string; border: string; text: string },
): void {
  const paddingX = 5 * pixelRatio;
  const height = 16 * pixelRatio;
  const width = ctx.measureText(text).width + 2 * paddingX;
  const left = Math.max(
    bbox.left + 2 * pixelRatio,
    Math.min(anchorX - width / 2, bbox.left + bbox.width - width - 2 * pixelRatio),
  );
  const top = centerY - height / 2;
  ctx.fillStyle = colors.background;
  ctx.fillRect(left, top, width, height);
  ctx.strokeStyle = colors.border;
  ctx.lineWidth = Math.max(1, pixelRatio * 0.75);
  ctx.strokeRect(left, top, width, height);
  ctx.fillStyle = colors.text;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  ctx.fillText(text, left + paddingX, centerY);
}

function cutoffMarkerPlugin(config: ResponseConfig): uPlot.Plugin {
  const cutoffs = config.topology === "band-pass"
    ? [config.lower_cutoff_hz, config.upper_cutoff_hz]
    : [config.cutoff_hz];
  const finiteCutoffs = cutoffs.filter((value): value is number => value !== undefined && Number.isFinite(value));
  return {
    hooks: {
      draw: (plot) => {
        const pixelRatio = window.devicePixelRatio || 1;
        const { ctx, bbox } = plot;
        ctx.save();
        ctx.font = `${9 * pixelRatio}px "DM Mono", monospace`;
        finiteCutoffs.forEach((cutoff, index) => {
          const x = Math.round(plot.valToPos(cutoff, "x", true));
          ctx.strokeStyle = "rgba(187, 112, 42, 0.82)";
          ctx.lineWidth = Math.max(1, pixelRatio);
          ctx.setLineDash([4 * pixelRatio, 4 * pixelRatio]);
          ctx.beginPath();
          ctx.moveTo(x, bbox.top);
          ctx.lineTo(x, bbox.top + bbox.height);
          ctx.stroke();
          ctx.setLineDash([]);
          drawCanvasLabel(
            ctx,
            bbox,
            "−3 dB",
            x,
            bbox.top + (11 + index % 2 * 19) * pixelRatio,
            pixelRatio,
            { background: "rgba(255, 252, 247, 0.96)", border: "rgba(207, 148, 91, 0.72)", text: "rgba(126, 67, 20, 0.98)" },
          );
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

function magnitudeFloorPlugin(result: ResponseResult): uPlot.Plugin {
  const crossings = magnitudeFloorCrossings(result);
  return {
    hooks: {
      draw: (plot) => {
        if (crossings.length === 0) return;
        const pixelRatio = window.devicePixelRatio || 1;
        const { ctx, bbox } = plot;
        ctx.save();
        ctx.font = `${9 * pixelRatio}px "DM Mono", monospace`;
        crossings.forEach((frequency, index) => {
          const x = Math.round(plot.valToPos(frequency, "x", true));
          const bottom = bbox.top + bbox.height;
          ctx.fillStyle = "rgba(22, 135, 121, 0.95)";
          ctx.beginPath();
          ctx.moveTo(x - 4 * pixelRatio, bottom - 7 * pixelRatio);
          ctx.lineTo(x + 4 * pixelRatio, bottom - 7 * pixelRatio);
          ctx.lineTo(x, bottom - pixelRatio);
          ctx.closePath();
          ctx.fill();
          drawCanvasLabel(
            ctx,
            bbox,
            "below view",
            x,
            bottom - (17 + index % 2 * 19) * pixelRatio,
            pixelRatio,
            { background: "rgba(247, 251, 248, 0.96)", border: "rgba(116, 166, 151, 0.62)", text: "rgba(45, 104, 91, 0.98)" },
          );
        });
        ctx.restore();
      },
    },
  };
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

function drawResponseChart(): void {
  if (!responseResult || !appliedConfig) return;
  const meta = chartMeta[activeChart];
  const values = displayChartData(responseResult);
  if (values.x.length !== values.y.length || values.x.length === 0) return;
  responsePlot?.destroy();
  responsePlot = undefined;
  ui.chartHost.replaceChildren();
  ui.chartTitle.textContent = meta.title;
  ui.chartSubtitle.textContent = meta.subtitle;
  ui.chartUnit.textContent = values.unit;
  ui.xAxisLabel.textContent = values.xLabel;
  ui.yAxisLabel.textContent = values.yLabel;
  ui.chartPanel.setAttribute("aria-labelledby", `tab-${activeChart}`);
  ui.chartHost.setAttribute("aria-label", `${meta.title}; ${makeResponseSummary(responseResult, appliedConfig, activeChart)}`);
  ui.responseChartSummary.textContent = makeResponseSummary(responseResult, appliedConfig, activeChart);

  const rect = ui.chartHost.getBoundingClientRect();
  const width = Math.max(220, Math.floor(rect.width || 660));
  const height = Math.max(230, Math.min(340, Math.round(width * 0.45)));
  const plugins = activeChart === "step" ? [] : [cutoffMarkerPlugin(appliedConfig)];
  if (activeChart === "magnitude") plugins.push(magnitudeFloorPlugin(responseResult));
  responsePlot = new uPlot({
    width,
    height,
    class: "ssfilt-plot",
    cursor: { drag: { x: false, y: false, setScale: false } },
    legend: { show: false },
    plugins,
    scales: {
      x: activeChart === "step"
        ? { time: false, distr: 1 }
        : { time: false, distr: 3, range: [values.x[0], values.x[values.x.length - 1]] },
      y: activeChart === "magnitude" ? { range: [MAGNITUDE_DISPLAY_FLOOR_DB, 5] } : {},
    },
    axes: [
      { stroke: "#687780", grid: { stroke: "rgba(21, 45, 57, 0.08)", width: 1 }, ticks: { stroke: "rgba(21, 45, 57, 0.14)", width: 1 }, font: "11px Inter, system-ui, sans-serif", size: 38 },
      { stroke: "#687780", grid: { stroke: "rgba(21, 45, 57, 0.08)", width: 1 }, ticks: { stroke: "rgba(21, 45, 57, 0.14)", width: 1 }, font: "11px Inter, system-ui, sans-serif", size: 48, side: 3 },
    ],
    series: [
      {},
      { label: meta.y, stroke: activeChart === "step" ? "#bfed4b" : "#168779", width: 2.5, points: { show: false } },
    ],
  }, [values.x, values.y], ui.chartHost);
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
  const initialMode = ui.initialMode.value as InitialMode;
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
  url.searchParams.set("initial_mode", ui.initialMode.value);
  if (ui.initialMode.value === "custom") {
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
  if (initialMode === "sample0" || initialMode === "custom") ui.initialMode.value = initialMode;
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
  const custom = ui.initialMode.value === "custom";
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
  ui.filteredPlaceholder.hidden = true;
  ui.filteredContent.hidden = false;
  ui.downloadCsv.hidden = false;
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
  filterPlot?.destroy();
  filterPlot = undefined;
  ui.filterChart.replaceChildren();
  showFilterPlaceholder("Your filtered trace will appear here", "Paste or generate a signal, then filter it with the active setup above.");
  if (appliedConfig) updateDataFilterContext(appliedConfig);
}

function loadCsvText(text: string): void {
  clearCsvError();
  updateCsvLineNumbers();
  try {
    const parsed = parseCsv(text);
    loadedCsv = parsed;
    filterResult = undefined;
    filterPlot?.destroy();
    filterPlot = undefined;
    ui.filterChart.replaceChildren();
    ui.csvStatus.textContent = `${parsed.rowCount.toLocaleString()} samples ready · ${shortNumber(parsed.times[0] ?? 0, 5)} to ${shortNumber(parsed.times.at(-1) ?? 0, 5)} seconds`;
    showFilterPlaceholder("Filtering your signal…", "Every irregular interval is being integrated with the active setup.");
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
  showFilterPlaceholder("Filtering your signal…", "Every irregular interval is being integrated with the active setup.");
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

function drawFilterChart(result: FilterResult): void {
  filterPlot?.destroy();
  filterPlot = undefined;
  ui.filterChart.replaceChildren();
  const time = numericSeries(result.time);
  const input = numericSeries(result.input);
  const output = numericSeries(result.output);
  const width = Math.max(220, Math.floor(ui.filterChart.getBoundingClientRect().width || 700));
  const height = Math.max(230, Math.min(380, Math.round(width * 0.4)));
  filterPlot = new uPlot({
    width,
    height,
    class: "ssfilt-plot",
    cursor: { drag: { x: false, y: false, setScale: false } },
    legend: { show: true, live: false },
    scales: { x: { time: false } },
    axes: [
      { stroke: "#687780", grid: { stroke: "rgba(21, 45, 57, 0.08)", width: 1 }, ticks: { stroke: "rgba(21, 45, 57, 0.14)", width: 1 }, font: "11px Inter, system-ui, sans-serif", size: 38 },
      { stroke: "#687780", grid: { stroke: "rgba(21, 45, 57, 0.08)", width: 1 }, ticks: { stroke: "rgba(21, 45, 57, 0.14)", width: 1 }, font: "11px Inter, system-ui, sans-serif", size: 48, side: 3 },
    ],
    series: [
      {},
      { label: "Input", stroke: "#89979d", width: 1.5, points: { show: true, size: 3, fill: "#f8faf7", stroke: "#89979d" } },
      { label: "Output", stroke: "#168779", width: 2.5, points: { show: false } },
    ],
  }, [time, input, output], ui.filterChart);
  const first = time[0] ?? 0;
  const last = time.at(-1) ?? first;
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

function watchResize(): void {
  if (typeof ResizeObserver === "undefined") return;
  resizeObserver = new ResizeObserver(() => {
    window.requestAnimationFrame(() => {
      if (responseResult) drawResponseChart();
      if (filterResult && !ui.filteredContent.hidden) drawFilterChart(filterResult);
    });
  });
  resizeObserver.observe(ui.chartHost);
  resizeObserver.observe(ui.filterChart);
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
ui.initialMode.addEventListener("change", () => {
  updateInitialMode();
  persistFilterConfiguration();
  if (loadedCsv) void runFilter();
});
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
  responsePlot?.destroy();
  filterPlot?.destroy();
  resizeObserver?.disconnect();
});
