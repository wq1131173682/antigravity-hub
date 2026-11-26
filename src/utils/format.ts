export function formatCompactNumber(num: number): string {
    if (num === 0) return '0';
    if (num < 1000 && num > -1000) return num.toString();

    const units = ['', 'k', 'M', 'G', 'T', 'P'];
    const absNum = Math.abs(num);
    const i = Math.floor(Math.log10(absNum) / 3);
    const value = num / Math.pow(1000, i);

    // Round to 1 decimal place if needed
    const formatted = value.toFixed(Math.abs(value) < 10 && i > 0 ? 1 : 0);
    return `${formatted.replace(/\.0$/, '')}${units[i]}`;
}
