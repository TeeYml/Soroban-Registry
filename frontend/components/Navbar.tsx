'use client';

import { Package, GitBranch, ChevronDown, BarChart2, Users, Menu, X, Layers, Search, Plus } from 'lucide-react';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { useState, useRef } from 'react';
import ThemeToggle from './ThemeToggle';

export default function Navbar() {
    const [isDropdownOpen, setIsDropdownOpen] = useState(false);
    const [menuOpenForPath, setMenuOpenForPath] = useState<string | null>(null);
    const dropdownTimeout = useRef<NodeJS.Timeout | null>(null);
    const pathname = usePathname();

    // Derive mobileMenuOpen: auto-closes when pathname changes
    const mobileMenuOpen = menuOpenForPath === pathname;
    const setMobileMenuOpen = (open: boolean) => setMenuOpenForPath(open ? pathname : null);

    const isActive = (href: string) => pathname === href;
    const isExploreActive = ['/publishers', '/stats', '/templates'].some(p => pathname.startsWith(p));

    const handleDropdownEnter = () => {
        if (dropdownTimeout.current) clearTimeout(dropdownTimeout.current);
        setIsDropdownOpen(true);
    };

    const handleDropdownLeave = () => {
        dropdownTimeout.current = setTimeout(() => setIsDropdownOpen(false), 150);
    };

    return (
        <nav className="nav-glow bg-background/80 backdrop-blur-2xl sticky top-0 z-50">
            <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
                <div className="flex items-center justify-between h-14">
                    {/* Logo */}
                    <Link href="/" className="flex items-center gap-2 group">
                        <div className="w-7 h-7 rounded-md bg-gradient-to-br from-primary to-secondary flex items-center justify-center shadow-sm shadow-primary/25">
                            <Package className="w-4 h-4 text-white" />
                        </div>
                        <span className="text-base font-bold text-foreground tracking-tight hidden sm:block">
                            Soroban<span className="text-primary">Registry</span>
                        </span>
                    </Link>

                    {/* Desktop Navigation – centered links */}
                    <div className="hidden md:flex items-center gap-0.5">
                        <Link
                            href="/contracts"
                            className={`px-3 py-1.5 rounded-md text-[13px] font-medium transition-all ${
                                isActive('/contracts')
                                    ? 'text-primary bg-primary/10'
                                    : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                            }`}
                        >
                            Browse
                        </Link>

                        {/* Explore Dropdown */}
                        <div
                            className="relative"
                            onMouseEnter={handleDropdownEnter}
                            onMouseLeave={handleDropdownLeave}
                        >
                            <button
                                className={`flex items-center gap-1 px-3 py-1.5 rounded-md text-[13px] font-medium transition-all focus:outline-none ${
                                    isExploreActive || isDropdownOpen
                                        ? 'text-primary bg-primary/10'
                                        : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                                }`}
                            >
                                Explore
                                <ChevronDown className={`w-3 h-3 transition-transform duration-200 ${isDropdownOpen ? 'rotate-180' : ''}`} />
                            </button>

                            {/* Compact dropdown */}
                            <div
                                className={`absolute top-full left-1/2 -translate-x-1/2 w-44 pt-1.5 transition-all duration-150 ${
                                    isDropdownOpen
                                        ? 'opacity-100 translate-y-0 pointer-events-auto'
                                        : 'opacity-0 -translate-y-1 pointer-events-none'
                                }`}
                            >
                                <div className="rounded-lg border border-border bg-card shadow-lg shadow-black/8 overflow-hidden">
                                    <div className="py-1">
                                        <Link
                                            href="/publishers"
                                            className={`flex items-center gap-2.5 px-3 py-2 text-[13px] transition-colors ${
                                                isActive('/publishers')
                                                    ? 'text-primary bg-primary/5'
                                                    : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                                            }`}
                                        >
                                            <Users className="w-3.5 h-3.5 text-primary/70" />
                                            Publishers
                                        </Link>
                                        <Link
                                            href="/stats"
                                            className={`flex items-center gap-2.5 px-3 py-2 text-[13px] transition-colors ${
                                                isActive('/stats')
                                                    ? 'text-primary bg-primary/5'
                                                    : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                                            }`}
                                        >
                                            <BarChart2 className="w-3.5 h-3.5 text-primary/70" />
                                            Statistics
                                        </Link>
                                        <Link
                                            href="/templates"
                                            className={`flex items-center gap-2.5 px-3 py-2 text-[13px] transition-colors ${
                                                isActive('/templates')
                                                    ? 'text-primary bg-primary/5'
                                                    : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                                            }`}
                                        >
                                            <Layers className="w-3.5 h-3.5 text-primary/70" />
                                            Templates
                                        </Link>
                                    </div>
                                </div>
                            </div>
                        </div>

                        <Link
                            href="/graph"
                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[13px] font-medium transition-all ${
                                isActive('/graph')
                                    ? 'text-primary bg-primary/10'
                                    : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                            }`}
                        >
                            <GitBranch className="w-3 h-3" />
                            Graph
                        </Link>
                    </div>

                    {/* Right actions */}
                    <div className="hidden md:flex items-center gap-1.5">
                        <ThemeToggle />
                        <Link
                            href="/publish"
                            className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-md bg-primary text-primary-foreground text-[13px] font-semibold btn-glow transition-all hover:brightness-110"
                        >
                            <Plus className="w-3.5 h-3.5" />
                            Publish
                        </Link>
                    </div>

                    {/* Mobile */}
                    <div className="flex md:hidden items-center gap-1.5">
                        <ThemeToggle />
                        <button
                            onClick={() => setMobileMenuOpen(!mobileMenuOpen)}
                            className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
                            aria-label="Toggle mobile menu"
                        >
                            {mobileMenuOpen ? <X className="w-5 h-5" /> : <Menu className="w-5 h-5" />}
                        </button>
                    </div>
                </div>

                {/* Mobile Navigation */}
                {mobileMenuOpen && (
                    <div className="md:hidden pb-4 pt-2 border-t border-border animate-fade-in-up">
                        <div className="flex flex-col gap-0.5">
                            {[
                                { href: '/contracts', label: 'Browse Contracts', icon: Search },
                                { href: '/publishers', label: 'Publishers', icon: Users },
                                { href: '/stats', label: 'Statistics', icon: BarChart2 },
                                { href: '/templates', label: 'Templates', icon: Layers },
                                { href: '/graph', label: 'Dependency Graph', icon: GitBranch },
                            ].map(({ href, label, icon: Icon }) => (
                                <Link
                                    key={href}
                                    href={href}
                                    className={`flex items-center gap-2.5 px-3 py-2.5 rounded-lg text-sm font-medium transition-all ${
                                        isActive(href)
                                            ? 'text-primary bg-primary/10'
                                            : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                                    }`}
                                >
                                    <Icon className="w-4 h-4" />
                                    {label}
                                </Link>
                            ))}
                            <div className="pt-2 mt-1 border-t border-border">
                                <Link
                                    href="/publish"
                                    className="flex items-center justify-center gap-1.5 px-4 py-2.5 rounded-lg bg-primary text-primary-foreground font-semibold text-sm btn-glow"
                                >
                                    <Plus className="w-4 h-4" />
                                    Publish Contract
                                </Link>
                            </div>
                        </div>
                    </div>
                )}
            </div>
        </nav>
    );
}
