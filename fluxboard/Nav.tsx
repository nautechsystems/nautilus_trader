// Navigation component
import { useState, useEffect } from 'react';
import { Link, useLocation } from 'react-router-dom';
import { cn } from './lib/utils';
import { Menu, X } from 'lucide-react';
import { colors } from '@/lib/tokens';
import { buildProfilePath, type PathProfile, type UiSurfaceContract } from './config/uiProfiles';

type NavProps = {
  profile: PathProfile;
  surface?: UiSurfaceContract;
};

export default function Nav({ profile, surface }: NavProps) {
  const location = useLocation();
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const activeSurface = surface;
  const protocol = window.location.protocol;
  const host = window.location.hostname;
  const links = activeSurface?.navLinks || [];
  const externalLinks = (activeSurface?.externalLinks || []).map(({ port, label, pathSuffix }) => ({
    url: `${protocol}//${host}:${port}${pathSuffix || ''}`,
    label,
  }));

  useEffect(() => {
    setMobileMenuOpen(false);
  }, [location.pathname]);

  const isActive = (path: string) => {
    const scopedPath = buildProfilePath(profile, path);
    if (path === '/') return location.pathname === scopedPath;
    return location.pathname.startsWith(scopedPath);
  };

  return (
    <nav
      className="sticky top-0 shrink-0 z-50 border-b"
      style={{ backgroundColor: colors.bg.surface, borderColor: colors.border.DEFAULT }}
      aria-label="Primary"
    >
      <div className="flex items-center w-full h-12 px-4 gap-4">
        <div className="flex items-center gap-2 mr-4 text-muted-foreground">
          <span
            className="text-sm font-semibold tracking-[0.08em] uppercase"
            style={{ color: colors.text.primary }}
          >
            flux
          </span>
        </div>

        {/* Desktop Nav */}
        <div className="hidden lg:flex items-center gap-1 overflow-x-auto no-scrollbar flex-1" data-testid="primary-nav-links">
          {links.map(({ path, label }) => {
            const active = isActive(path);
            const scopedPath = buildProfilePath(profile, path);
            return (
              <Link
                key={path}
                to={scopedPath}
                className={cn(
                  'nav-link nav-link--primary relative inline-flex items-center h-8 px-3 text-[12px] font-semibold tracking-tight transition-colors duration-150 border-b-2',
                  active && 'nav-link--active',
                )}
                aria-current={active ? 'page' : undefined}
                data-active={active ? 'true' : 'false'}
              >
                {label}
              </Link>
            );
          })}

          {externalLinks.length > 0 && (
            <>
              <div className="h-4 w-px mx-3" style={{ backgroundColor: colors.border.DEFAULT }} />

              <div className="nav-link-external-group inline-flex items-center gap-1 pl-2">
                {externalLinks.map(({ url, label }) => (
                  <a
                    key={url}
                    href={url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="nav-link nav-link--external inline-flex items-center h-8 px-3 text-[12px] font-semibold tracking-tight transition-colors border-b-2 border-transparent"
                    data-nav-kind="external"
                  >
                    {label} ↗
                  </a>
                ))}
              </div>
            </>
          )}
        </div>

        {/* Mobile Menu Toggle */}
        <button
          className="lg:hidden ml-auto rounded-[3px] p-2 transition-colors border"
          onClick={() => setMobileMenuOpen(!mobileMenuOpen)}
          style={{ color: colors.text.muted, backgroundColor: colors.bg.hover, borderColor: colors.border.DEFAULT }}
          aria-label="Toggle navigation"
        >
          {mobileMenuOpen ? <X className="w-5 h-5" /> : <Menu className="w-5 h-5" />}
        </button>
      </div>

      {/* Mobile Nav Overlay */}
      {mobileMenuOpen && (
        <div
          className="absolute top-12 left-0 right-0 p-3 flex flex-col gap-2 lg:hidden border-b"
          style={{ backgroundColor: colors.bg.base, borderColor: colors.border.DEFAULT }}
        >
          {links.map(({ path, label }) => {
            const active = isActive(path);
            const scopedPath = buildProfilePath(profile, path);
            return (
              <Link
                key={path}
                to={scopedPath}
                className="px-3 py-2 rounded-[3px] text-sm font-semibold border"
                style={{
                  backgroundColor: active ? colors.bg.hover : colors.bg.surface,
                  color: active ? colors.text.primary : colors.text.muted,
                  borderColor: active ? colors.accent.muted : colors.border.DEFAULT,
                }}
              >
                {label}
              </Link>
            );
          })}
          {externalLinks.length > 0 && (
            <>
              <div className="h-px my-2" style={{ backgroundColor: colors.border.DEFAULT }} />
              {externalLinks.map(({ url, label }) => (
                <a
                  key={url}
                  href={url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="nav-link nav-link--external px-3 py-2 rounded-[3px] text-sm font-semibold border"
                  data-nav-kind="external"
                  style={{
                    color: colors.text.muted,
                    backgroundColor: colors.bg.hover,
                    borderColor: colors.border.DEFAULT,
                  }}
                >
                  {label} ↗
                </a>
              ))}
            </>
          )}
        </div>
      )}
    </nav>
  );
}
