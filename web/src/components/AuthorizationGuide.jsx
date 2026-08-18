import { useEffect, useRef } from "react";
import { ArrowLeft, ArrowSquareOut } from "@phosphor-icons/react";
import image163Step1 from "../assets/help/mail-authorization/163-step-1.png";
import image163Step2 from "../assets/help/mail-authorization/163-step-2.png";
import imageGmailAppPassword from "../assets/help/mail-authorization/gmail-app-password.png";
import imageQqStep1 from "../assets/help/mail-authorization/qq-step-1.png";
import imageQqStep2 from "../assets/help/mail-authorization/qq-step-2.png";
import { IconButton } from "./IconButton.jsx";

const guides = {
  "163": {
    title: "163 邮箱授权码教程",
    returnLabel: "返回连接163 邮箱",
    introduction: "在 163 邮箱网页版开启 IMAP/SMTP，并生成客户端专用授权密码。",
    steps: [
      {
        text: "登录 163 邮箱网页版，点击页面顶部的“设置”，在菜单中选择“POP3/SMTP/IMAP”。",
        image: image163Step1,
        alt: "163 邮箱设置菜单中的 POP3/SMTP/IMAP 入口",
        caption: "从“设置”菜单进入 POP3/SMTP/IMAP。",
      },
      {
        text: "确认“IMAP/SMTP服务”显示“已开启”。然后向下找到“授权密码管理”，点击“新增授权密码”，并按提示完成安全验证。",
        image: image163Step2,
        alt: "163 邮箱已开启 IMAP/SMTP 服务并显示新增授权密码按钮",
        caption: "服务必须保持开启，再新增授权密码。",
      },
      {
        text: "复制新生成的授权码，返回 Mine Mail，将完整 163 邮箱地址和授权码填入连接表单。不要填写网页版登录密码。",
      },
    ],
  },
  qq: {
    title: "QQ 邮箱授权码教程",
    returnLabel: "返回连接QQ 邮箱",
    introduction: "在 QQ 邮箱网页版的安全设置中开启邮件服务，并生成客户端授权码。",
    steps: [
      {
        text: "登录 QQ 邮箱网页版，点击右上角的“设置”，再点击左侧边栏中的“账号与安全”。",
        image: imageQqStep1,
        alt: "QQ 邮箱设置页中的设置按钮和账号与安全入口",
        caption: "先进入“设置”，再打开“账号与安全”。",
      },
      {
        text: "在“账号与安全”页面左侧点击“安全设置”。找到“POP3/IMAP/SMTP/Exchange/CardDAV 服务”，确认状态为“已开启”，然后点击“生成授权码”。",
        image: imageQqStep2,
        alt: "QQ 邮箱安全设置中的邮件服务和生成授权码按钮",
        caption: "在“安全设置”中确认服务开启并生成授权码。",
      },
      {
        text: "完成安全验证并复制授权码，返回 Mine Mail，将完整 QQ 邮箱地址和授权码填入连接表单。不要填写 QQ 登录密码。",
      },
    ],
  },
  gmail: {
    title: "Gmail 应用专用密码教程",
    returnLabel: "返回连接 Gmail",
    introduction:
      "先为 Google 账户开启两步验证，再创建一个仅供 Mine Mail 使用的应用专用密码。",
    externalUrl: "https://myaccount.google.com/apppasswords",
    externalLabel: "打开 Google 应用专用密码",
    steps: [
      {
        text: "确认 Google 账户已开启两步验证。未开启时，Google 不会提供应用专用密码。",
      },
      {
        text: "打开 Google 应用专用密码页面并登录账户，在“应用名称”中输入 Mine Mail，然后点击“创建”。",
        image: imageGmailAppPassword,
        alt: "Google 应用专用密码页面中的应用名称输入框和创建按钮",
        caption: "输入应用名称后点击“创建”。",
      },
      {
        text: "复制 Google 生成的 16 位应用专用密码，返回 Mine Mail，将完整 Gmail 或 Google Workspace 地址和该密码填入连接表单。不要填写普通 Google 登录密码。",
      },
    ],
    securityTitle: "请妥善保管应用专用密码",
    securityText:
      "应用专用密码与密码同样敏感，而且只显示一次。请勿发送给他人或保留包含密码的截图；如果怀疑泄露，请立即在 Google 账户中撤销并重新生成。",
  },
};

export function AuthorizationGuide({ provider, onBack, onOpenExternalLink }) {
  const guide = guides[provider];
  const pageRef = useRef(null);

  useEffect(() => {
    pageRef.current?.focus({ preventScroll: true });
  }, [provider]);

  if (!guide) return null;

  const titleId = `authorization-guide-${provider}-title`;

  return (
    <section
      ref={pageRef}
      className="settings-page settings-page--flow authorization-guide-page"
      aria-labelledby={titleId}
      tabIndex={-1}
    >
      <header className="settings-flow-heading">
        <IconButton label={guide.returnLabel} onClick={onBack}>
          <ArrowLeft size={18} />
        </IconButton>
        <span>
          <p className="eyebrow">账户帮助</p>
          <h3 id={titleId}>{guide.title}</h3>
          <p>{guide.introduction}</p>
        </span>
      </header>

      <article className="authorization-guide-content">
        {guide.externalUrl ? (
          <a
            className="secondary-button authorization-guide-external"
            href={guide.externalUrl}
            onClick={(event) => {
              event.preventDefault();
              onOpenExternalLink?.(guide.externalUrl);
            }}
          >
            {guide.externalLabel}
            <ArrowSquareOut size={16} weight="bold" aria-hidden="true" />
          </a>
        ) : null}
        <ol className="authorization-guide-steps">
          {guide.steps.map((step, index) => (
            <li key={step.text} className="authorization-guide-step">
              <p>{step.text}</p>
              {step.image ? (
                <figure className="authorization-guide-figure">
                  <img src={step.image} alt={step.alt} draggable="false" />
                  <figcaption>
                    图 {index + 1}：{step.caption}
                  </figcaption>
                </figure>
              ) : null}
            </li>
          ))}
        </ol>

        <aside className="authorization-guide-security">
          <strong>{guide.securityTitle || "请妥善保管授权码"}</strong>
          <p>
            {guide.securityText ||
              "授权码与密码同样敏感。请勿发送给他人或保留包含授权码的截图；如果怀疑泄露，请立即在邮箱网页端撤销并重新生成。"}
          </p>
        </aside>
      </article>
    </section>
  );
}
