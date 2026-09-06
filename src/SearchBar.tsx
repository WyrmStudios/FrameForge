interface SearchBarProps {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  className?: string;
}

export default function SearchBar({ value, onChange, placeholder, className = "search-box" }: SearchBarProps) {
  return (
    <input
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
