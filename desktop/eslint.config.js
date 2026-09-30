import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["ui/js/", "src-tauri/"] },
  js.configs.recommended,
  ...tseslint.configs.strict,
  {
    // Node scripts; the rest runs in the window, typed through tsconfig.json.
    files: ["scripts/**", "*.js"],
    languageOptions: { globals: { console: "readonly", process: "readonly", URL: "readonly" } },
  },
);
