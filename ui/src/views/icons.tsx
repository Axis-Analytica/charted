import { Binary, Calendar, CalendarClock, Clock, Hash, Percent, Sigma, SquareFunction, ToggleLeft, Type } from "lucide-react";
import type { Column } from "../bridge/generated/Column";
import type { DataType } from "../bridge/generated/DataType";

export function DataTypeIcon({ type, size = 14 }: { type: DataType; size?: number }) {
  switch (type) {
    case "String": return <Type size={size} />;
    case "Int64": return <Hash size={size} />;
    case "Double":
    case "Decimal": return <Percent size={size} />;
    case "Date": return <Calendar size={size} />;
    case "DateTime":
    case "DateTimeTz": return <CalendarClock size={size} />;
    case "Time": return <Clock size={size} />;
    case "Boolean": return <ToggleLeft size={size} />;
    default: return <Binary size={size} />;
  }
}

export function ColumnIcon({ column, size = 14 }: { column: Column; size?: number }) {
  return column.kind.kind === "calculated" ? <SquareFunction size={size} /> : <DataTypeIcon type={column.dataType} size={size} />;
}

export function MeasureIcon({ size = 14 }: { size?: number }) {
  return <Sigma size={size} />;
}
