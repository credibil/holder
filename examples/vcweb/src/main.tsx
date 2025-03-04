import React from "react"

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import ReactDOM from "react-dom/client"
import { RecoilRoot } from "recoil";

import App from "./App"

const queryClient = new QueryClient();

ReactDOM.createRoot(document.getElementById("root")!).render(
    < React.StrictMode >
        <RecoilRoot>
            <QueryClientProvider client={queryClient}>
                <App />
            </QueryClientProvider>
        </RecoilRoot>
    </React.StrictMode >,
)
