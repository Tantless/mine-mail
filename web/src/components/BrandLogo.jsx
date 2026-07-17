import foxLogo from "../assets/brand/mine-mail-fox.png";

export function BrandLogo({ className = "", alt = "" }) {
  const classes = ["brand-logo", className].filter(Boolean).join(" ");

  return <img className={classes} src={foxLogo} alt={alt} draggable="false" />;
}
