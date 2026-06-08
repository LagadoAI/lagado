// Layout wrapper for most pages
 
import React from "react";
import { Header } from "./Header";
 
interface LayoutProps {
  children: React.ReactNode;
  title: string;
  fullWidth?: boolean;
}
 
export function Layout({ children, title, fullWidth }: LayoutProps) {
  return (
    <div className="min-h-screen bg-lagado-bg flex flex-col">
      <Header title={title} />
      <main className={fullWidth ? "flex-1" : "flex-1 max-w-7xl mx-auto w-full"}>
        {children}
      </main>
    </div>
  );
}
 
