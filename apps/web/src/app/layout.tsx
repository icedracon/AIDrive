import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "AIDrive — Your work, across every AI",
  description: "A workspace for files, project memory and AI handoffs.",
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
