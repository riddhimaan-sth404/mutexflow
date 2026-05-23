console.warn(
  "[inject-env] This script is deprecated. config.js now reads BACKEND_URL from process.env at runtime.\n" +
  "Set the environment variable before launching the app instead:\n" +
  "  $env:BACKEND_URL='http://your-server'; npm start\n" +
  "Or pass it via electron-builder: --extraEnv.BACKEND_URL=...\n" +
  "No changes were made."
);
