import type { NextConfig } from "next";

const apiUrl = process.env.AIDRIVE_WEB_API_URL ?? "http://127.0.0.1:8080";

// AIDRIVE_WEB_EXPORT=1 builds static files (out/) that aidrive-api serves
// itself, so an installed copy needs no Node.js; the API answers /api/* there.
const staticExport = process.env.AIDRIVE_WEB_EXPORT === "1";

const nextConfig: NextConfig = staticExport
  ? { output: "export", images: { unoptimized: true } }
  : {
      async rewrites() {
        return [{ source: "/api/:path*", destination: `${apiUrl}/:path*` }];
      },
    };

export default nextConfig;
