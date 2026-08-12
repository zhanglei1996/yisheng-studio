import React from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { App } from "./App";
import { ConfigProvider, theme } from "antd";
import zhCN from "antd/locale/zh_CN";
import "antd/dist/reset.css";
import "./styles.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 60_000,
      gcTime: 15 * 60_000,
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
});

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <ConfigProvider
        locale={zhCN}
        theme={{
          algorithm: theme.darkAlgorithm,
          token: {
            colorPrimary: "#2f7dff",
            colorBgBase: "#0b0d10",
            colorBgContainer: "#12161b",
            colorBgElevated: "#171c22",
            colorBorder: "#2a3038",
            colorText: "#f3f5f7",
            colorTextSecondary: "#9ca3af",
            borderRadius: 6,
            controlHeight: 34,
            fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", "Helvetica Neue", sans-serif',
          },
          components: {
            Button: { primaryShadow: "none", defaultBg: "#151a20", defaultBorderColor: "#303740" },
            Checkbox: { colorPrimary: "#2f7dff" },
            Input: { activeShadow: "0 0 0 2px rgba(47,125,255,.16)", activeBorderColor: "#4a8eff" },
            Modal: { contentBg: "#12161b", headerBg: "#12161b", footerBg: "#101419" },
            Progress: { defaultColor: "#2f7dff", remainingColor: "#262c33" },
            Select: { activeOutlineColor: "rgba(47,125,255,.16)", optionSelectedBg: "#262d36" },
            Segmented: { trackBg: "#0f1317", itemSelectedBg: "#262d36" },
            Slider: { trackBg: "#2f7dff", trackHoverBg: "#4a8eff", handleColor: "#2f7dff" },
            Switch: { colorPrimary: "#2f7dff", colorPrimaryHover: "#4a8eff" },
          },
        }}
      >
        <BrowserRouter>
          <App />
        </BrowserRouter>
      </ConfigProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
