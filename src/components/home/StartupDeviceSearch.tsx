import {
  AlertCircle,
  ArrowRight,
  CheckCircle2,
  Mouse,
  RefreshCw,
  Search,
} from "lucide-react";
import { useMemo, useState } from "react";

export interface StartupDetectedHidMouse {
  hid_id: string;
  vid: number;
  pid: number;
  device_type: number;
  manufacturer_id: number;
  type_name: string;
}

export interface StartupHidMouseMonitorSnapshot {
  matched_devices: StartupDetectedHidMouse[];
  last_error: string | null;
  updated_at_unix_ms: number | null;
}

interface StartupDeviceSearchProps {
  snapshot: StartupHidMouseMonitorSnapshot;
  onRefreshSearch: () => void;
  onEnterMainPage: () => void;
}

const tips = [
  "请确保鼠标设备已开机，并靠近当前电脑",
  "如果长时间未找到，请检查设备电量或重新配对",
  "当前页面为临时测试入口，可直接跳转到主页面",
];

const StartupDeviceSearch: React.FC<StartupDeviceSearchProps> = ({
  snapshot,
  onRefreshSearch,
  onEnterMainPage,
}) => {
  const [refreshSeed, setRefreshSeed] = useState(0);
  const activeTip = useMemo(() => tips[refreshSeed % tips.length], [refreshSeed]);
  const hasMatchedDevices = snapshot.matched_devices.length > 0;
  const hasError = Boolean(snapshot.last_error);
  const statusTitle = hasMatchedDevices
    ? "已找到匹配鼠标"
    : hasError
      ? "设备搜索失败"
      : "正在搜索设备...";
  const statusSubtitle = hasMatchedDevices
    ? `已识别 ${snapshot.matched_devices.length} 台匹配设备，正在准备进入主页面`
    : hasError
      ? "后台检测线程返回了错误信息"
      : "请确保设备已开启并尽量靠近";
  const lastUpdatedText = snapshot.updated_at_unix_ms
    ? new Date(snapshot.updated_at_unix_ms).toLocaleTimeString("zh-CN", {
        hour12: false,
      })
    : null;

  return (
    <div className="relative flex h-full w-full overflow-hidden bg-white">
      <div className="absolute inset-x-0 top-0 h-7 bg-[#dff5ff]" />

      <div className="relative flex flex-1 items-center justify-center px-6 py-10">
        <div className="flex w-full max-w-3xl flex-col items-center text-center">
          <div className="relative mb-10 flex h-56 w-56 items-center justify-center">
            <div
              key={refreshSeed}
              className="absolute h-44 w-44 animate-ping rounded-full border border-sky-200/80"
              style={{ animationDuration: "2.4s" }}
            />
            <div className="absolute h-44 w-44 rounded-full border border-sky-100" />
            <div className="absolute h-16 w-16 rounded-full bg-sky-500 shadow-[0_0_30px_rgba(59,130,246,0.35)]" />
            <div className="relative flex h-16 w-16 items-center justify-center rounded-full border-2 border-white bg-sky-500 text-white shadow-lg">
              <Search className="h-7 w-7" />
            </div>
          </div>

          <div className="mb-8 space-y-2">
            <div
              className={`inline-flex items-center gap-2 rounded-full px-4 py-1.5 text-sm font-medium ${
                hasMatchedDevices
                  ? "border border-emerald-100 bg-emerald-50 text-emerald-700"
                  : hasError
                    ? "border border-rose-100 bg-rose-50 text-rose-700"
                    : "border border-sky-100 bg-sky-50 text-sky-700"
              }`}
            >
              {hasMatchedDevices ? (
                <CheckCircle2 className="h-4 w-4" />
              ) : hasError ? (
                <AlertCircle className="h-4 w-4" />
              ) : (
                <Mouse className="h-4 w-4" />
              )}
              <span>
                {hasMatchedDevices ? "匹配鼠标已发现" : "正在搜索鼠标设备"}
              </span>
            </div>
            <h1 className="text-3xl font-semibold text-slate-800">{statusTitle}</h1>
            <p className="text-sm text-slate-500">{statusSubtitle}</p>
          </div>

          <p className="mb-8 text-base text-slate-600">{activeTip}</p>

          {hasMatchedDevices ? (
            <div className="mb-8 grid w-full max-w-2xl gap-3 text-left sm:grid-cols-2">
              {snapshot.matched_devices.map((device) => (
                <div
                  key={device.hid_id}
                  className="rounded-2xl border border-emerald-100 bg-emerald-50/70 px-4 py-3 shadow-[0_12px_30px_rgba(16,185,129,0.08)]"
                >
                  <div className="text-sm font-semibold text-slate-800">
                    {device.type_name || "匹配鼠标"}
                  </div>
                  <div className="mt-1 text-xs text-slate-500">
                    VID_{device.vid.toString(16).toUpperCase().padStart(4, "0")}
                    {" / "}
                    PID_{device.pid.toString(16).toUpperCase().padStart(4, "0")}
                  </div>
                  <div className="mt-1 text-xs text-slate-500">
                    类型 {device.device_type}，厂商 {device.manufacturer_id}
                  </div>
                </div>
              ))}
            </div>
          ) : null}

          {hasError ? (
            <div className="mb-8 w-full max-w-2xl rounded-2xl border border-rose-100 bg-rose-50 px-4 py-3 text-left text-sm text-rose-700">
              {snapshot.last_error}
            </div>
          ) : null}

          {lastUpdatedText ? (
            <div className="mb-8 text-xs text-slate-400">
              后台最近一次扫描时间 {lastUpdatedText}，默认每 5 秒刷新一次
            </div>
          ) : null}

          <div className="flex flex-wrap items-center justify-center gap-3">
            <button
              type="button"
              onClick={() => {
                setRefreshSeed((prev) => prev + 1);
                onRefreshSearch();
              }}
              className="inline-flex items-center gap-2 rounded-lg border border-sky-200 bg-sky-50 px-4 py-2.5 text-sm font-medium text-sky-700 transition hover:border-sky-300 hover:bg-sky-100"
            >
              <RefreshCw className="h-4 w-4" />
              <span>重新搜索</span>
            </button>

            <button
              type="button"
              onClick={onEnterMainPage}
              className="inline-flex items-center gap-2 rounded-lg bg-slate-900 px-4 py-2.5 text-sm font-medium text-white shadow-[0_10px_24px_rgba(15,23,42,0.18)] transition hover:bg-slate-800"
            >
              <ArrowRight className="h-4 w-4" />
              <span>{hasMatchedDevices ? "进入主页面" : "临时进入主页面"}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

export default StartupDeviceSearch;
