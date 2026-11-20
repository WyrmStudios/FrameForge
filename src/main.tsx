import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { APP_TITLE } from "./constants/app";
import "./styles/tailwind.css";

document.title = APP_TITLE;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
