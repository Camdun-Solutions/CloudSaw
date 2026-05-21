import { useId, type SelectHTMLAttributes } from "react";

type Option<V extends string> = {
  value: V;
  label: string;
};

type SelectProps<V extends string> = Omit<
  SelectHTMLAttributes<HTMLSelectElement>,
  "onChange" | "value"
> & {
  label: string;
  value: V;
  options: Option<V>[];
  onChange: (next: V) => void;
  description?: string;
};

export default function Select<V extends string>({
  label,
  value,
  options,
  onChange,
  description,
  className = "",
  id,
  ...rest
}: SelectProps<V>) {
  const reactId = useId();
  const selectId = id ?? `sel-${reactId}`;
  const descId = `${selectId}-desc`;

  return (
    <div className={`flex flex-col gap-1.5 ${className}`}>
      <label
        htmlFor={selectId}
        className="text-small font-medium text-saw-grey-700"
      >
        {label}
      </label>
      <select
        id={selectId}
        value={value}
        onChange={(e) => onChange(e.target.value as V)}
        aria-describedby={description ? descId : undefined}
        className={[
          "block w-full rounded-card border border-saw-grey-300 bg-saw-white px-3 py-2",
          "text-body text-saw-grey-900",
          "focus:outline-none focus:ring-2 focus:ring-saw-orange focus:ring-offset-1",
        ].join(" ")}
        {...rest}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      {description ? (
        <p id={descId} className="text-small text-saw-grey-500">
          {description}
        </p>
      ) : null}
    </div>
  );
}
