export async function copyHistory(text, fallbackLabel, closeLabel) {
    try {
        await navigator.clipboard.writeText(text);
        return true;
    } catch {
        const dialog = document.createElement('dialog');
        const label = document.createElement('p');
        label.textContent = fallbackLabel;
        const field = document.createElement('textarea');
        field.value = text;
        field.readOnly = true;
        field.setAttribute('aria-label', fallbackLabel);
        field.style.cssText = 'width:70vw;height:50vh;display:block';
        const close = document.createElement('button');
        close.textContent = closeLabel;
        close.onclick = () => dialog.close();
        dialog.append(label, field, close);
        dialog.addEventListener('close', () => dialog.remove(), { once: true });
        document.body.append(dialog);
        dialog.showModal();
        field.focus();
        field.select();
        return false;
    }
}
