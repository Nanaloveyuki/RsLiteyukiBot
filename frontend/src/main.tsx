import ReactDOM from "react-dom/client";
import { HashRouter } from "react-router-dom";

import App from "@/App";
import { Provider } from "@/provider";
import "@/styles/globals.css";

ReactDOM.createRoot(document.getElementById("app")!).render(
  <HashRouter>
    <Provider>
      <App />
    </Provider>
  </HashRouter>,
);
