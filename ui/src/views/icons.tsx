import { Binary, Calendar, CalendarClock, Clock, Hash, Layers, Percent, Sigma, SquareFunction, Table2, TableProperties, ToggleLeft, Type } from "lucide-react";
import type { Column } from "../bridge/generated/Column";
import type { DataType } from "../bridge/generated/DataType";
import type { Table } from "../bridge/generated/Table";
import { useT } from "../i18n";

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

/** A table: calculated tables (from a DAX expression) and calculation groups have their
 * own icons. */
export function TableIcon({ table, size = 14, className }: { table: Table; size?: number; className?: string }) {
  const { t } = useT();
  if (!table.calculated) return <Table2 size={size} className={className} />;
  if (table.calculationGroup)
    return (
      <span className={`table-icon-calc table-icon-group ${className ?? ""}`} title={t("Calculation group")} aria-label={t("Calculation group")} role="img">
        <Layers size={size} />
      </span>
    );
  return (
    <span className={`table-icon-calc ${className ?? ""}`} title={t("Calculated table")} aria-label={t("Calculated table")} role="img">
      <TableProperties size={size} />
    </span>
  );
}

export function MeasureIcon({ size = 14 }: { size?: number }) {
  return <Sigma size={size} />;
}
