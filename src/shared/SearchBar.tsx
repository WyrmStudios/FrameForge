const SEARCH_BOX =
  "flex-1 bg-background border border-border rounded-6 text-foreground px-2.5 py-1.5 text-13 outline-none focus:border-accent";

interface SearchBarProps {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  className?: string;
  id?: string;
}

export default function SearchBar({ value, onChange, placeholder, className = SEARCH_BOX, id }: SearchBarProps) {
  return (
    <input
      id={id}
      className={className}
      placeholder={placeholder}
      value={value}
      onChange={event => onChange(event.target.value)}
      onKeyDown={event => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        onChange("");
        event.currentTarget.blur();
      }}
    />
  );
}
